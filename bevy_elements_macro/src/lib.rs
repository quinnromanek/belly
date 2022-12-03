use proc_macro2::TokenStream;
use quote::*;
extern crate proc_macro;
use syn::{parse_macro_input, spanned::Spanned, Error, Expr, ExprPath, ItemFn};
use syn_rsx::{parse, Node, NodeAttribute};

fn create_single_command_stmt(expr: &ExprPath) -> TokenStream {
    let component_span = expr.span();
    if let Some(component) = expr.path.get_ident() {
        if component.to_string().chars().next().unwrap().is_uppercase() {
            quote_spanned! {component_span=>
                c.insert(#component::default());
            }
        } else {
            quote_spanned! {component_span=>
                c.insert(#component);
            }
        }
    } else {
        Error::new(component_span, "Invalid components declaration").into_compile_error()
    }
}

fn create_command_stmts(expr: &Expr) -> TokenStream {
    let with_body = match expr {
        Expr::Path(path) => create_single_command_stmt(path),
        Expr::Tuple(components) => {
            let mut components_expr = quote! {};
            for component_expr in components.elems.iter() {
                let component_span = component_expr.span();
                if let Expr::Path(component) = component_expr {
                    let component_expr = create_single_command_stmt(component);
                    components_expr = quote_spanned! {component_span=>
                        #components_expr
                        #component_expr
                    };
                } else {
                    return Error::new(component_span, "Invalid component name")
                        .into_compile_error();
                }
            }
            components_expr
        }
        _ => {
            return Error::new(expr.span(), "Invalid components declaration").into_compile_error();
        }
    };
    let expr_span = expr.span();
    quote_spanned! {expr_span=>
        __ctx.attributes.add(::bevy_elements_core::attributes::Attribute::from_commands("with", ::std::boxed::Box::new(move |c| {
            #with_body
        })));
    }
}

fn create_attr_stmt(attr: &NodeAttribute) -> TokenStream {
    let attr_name = attr.key.to_string();
    match &attr.value {
        None => {
            return quote! {
                __ctx.attributes.add(::bevy_elements_core::attributes::Attribute::new(
                    #attr_name.into(),
                    ::bevy_elements_core::attributes::AttributeValue::Empty
                ));
            };
        }
        Some(attr_value) => {
            let attr_value = attr_value.as_ref();
            let attr_span = attr_value.span();
            if attr_name == "with" {
                return create_command_stmts(attr_value);
            } else {
                return quote_spanned! {attr_span=>
                    __ctx.attributes.add(::bevy_elements_core::attributes::Attribute::new(
                        #attr_name.into(),
                        (#attr_value).into()
                    ));
                };
            }
        }
    }
}

fn walk_nodes<'a>(element: &'a Node, create_entity: bool) -> TokenStream {
    let mut children = quote! {};
    let mut parent = if create_entity {
        quote! { let __parent = __world.spawn_empty().id(); }
    } else {
        quote! {}
    };
    if let Node::Element(element) = element {
        let mut parent_defined = false;
        for attr in element.attributes.iter() {
            if let Node::Block(entity) = attr {
                let entity_span = entity.value.span();
                let entity = entity.value.as_ref();
                if parent_defined {
                    return Error::new(entity_span, "Entity already provided by entity attribute")
                        .into_compile_error();
                }
                parent_defined = true;
                parent = quote! {
                    let __parent = #entity;
                };
            } else if let Node::Attribute(attr) = attr {
                let attr_name = attr.key.to_string();
                if &attr_name == "entity" {
                    let attr_span = attr.key.span();
                    if parent_defined {
                        return Error::new(attr_span, "Entity already provided by braced block")
                            .into_compile_error();
                    }
                    parent_defined = true;
                    let attr_value = attr.value.as_ref();
                    if attr_value.is_none() {
                        return Error::new(attr_span, "Attriute entity should has a value")
                            .into_compile_error();
                    }
                    let entity = attr_value.unwrap().as_ref();
                    parent = quote_spanned! { attr_span=>
                        let __parent = #entity;
                    };
                } else {
                    let attr_stmt = create_attr_stmt(attr);
                    children = quote! {
                        #children
                        #attr_stmt
                    };
                }
            }
        }
        for child in element.children.iter() {
            match child {
                Node::Element(_) => {
                    let expr = walk_nodes(child, true);
                    children = quote! {
                        #children
                        __ctx.children.push( #expr );
                    };
                }
                Node::Text(text) => {
                    let text = text.value.as_ref();
                    children = quote! {
                        #children
                        __ctx.children.push(
                            __world.spawn(::bevy::prelude::TextBundle {
                                text: ::bevy::prelude::Text::from_section(
                                    #text,
                                    ::std::default::Default::default()
                                ),
                                ..default()
                            })
                            .insert(::bevy_elements_core::Element::inline())
                            .id()
                        );
                    };
                }
                Node::Block(block) => {
                    let block = block.value.as_ref();
                    let block_span = block.span();
                    children = quote_spanned! { block_span=>
                        #children
                        let __node = __world.spawn_empty().id();
                        for __child in #block.into_content(__node, __world).iter() {
                            __ctx.children.push( __child.clone() );
                        }
                    }
                }
                _ => (),
            };
        }

        let tag = syn::Ident::new(&element.name.to_string(), element.span());
        quote! {
            {
                #parent
                let mut __ctx = ::bevy_elements_core::eml::build::ElementContextData::new(__parent);

                #children
                let __builder = ::bevy_elements_core::Elements::#tag();
                ::bevy_elements_core::build_element(
                    __world,
                    __ctx,
                    __builder,
                );
                __parent
            }
        }
    } else {
        quote! {}
    }
}

#[proc_macro]
pub fn eml(tree: proc_macro::TokenStream) -> proc_macro::TokenStream {
    match parse(tree.into()) {
        Err(err) => err.to_compile_error().into(),
        Ok(nodes) => {
            let body = walk_nodes(&nodes[0], false);
            // nodes[0]
            let wraped = quote! {
                ::bevy_elements_core::ElementsBuilder::new(
                    move |
                        __world: &mut ::bevy::prelude::World,
                        __parent: ::bevy::prelude::Entity
                    | {
                        #body;
                    }
            )};
            proc_macro::TokenStream::from(wraped)
        }
    }
}

// #[proc_macro_attribute]
// pub fn widget(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
//     if !args.is_empty() {
//         eprintln!("widget macro do not take any args");
//     }
//     let func = parse_macro_input!(input as ItemFn);

//     let result = quote! {
//         #func
//     };
//     proc_macro::TokenStream::from( result )
//     // let result = {

//     // }
// }
