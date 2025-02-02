/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use cg::{self, WhereClause};
use darling::util::Override;
use quote::{ToTokens, Tokens};
use syn::{self, Data};
use synstructure::{BindingInfo, Structure, VariantInfo};

pub fn derive(input: syn::DeriveInput) -> Tokens {
    let name = &input.ident;
    let trait_path = parse_quote!(::style_traits::ToCss);
    let (impl_generics, ty_generics, mut where_clause) =
        cg::trait_parts(&input, &trait_path);

    let input_attrs = cg::parse_input_attrs::<CssInputAttrs>(&input);
    if let Data::Enum(_) = input.data {
        assert!(input_attrs.function.is_none(), "#[css(function)] is not allowed on enums");
        assert!(!input_attrs.comma, "#[css(comma)] is not allowed on enums");
    }
    let s = Structure::new(&input);

    let match_body = s.each_variant(|variant| {
        derive_variant_arm(variant, &mut where_clause)
    });

    let mut impls = quote! {
        impl #impl_generics ::style_traits::ToCss for #name #ty_generics #where_clause {
            #[allow(unused_variables)]
            #[inline]
            fn to_css<W>(
                &self,
                dest: &mut ::style_traits::CssWriter<W>,
            ) -> ::std::fmt::Result
            where
                W: ::std::fmt::Write
            {
                match *self {
                    #match_body
                }
            }
        }
    };

    if input_attrs.derive_debug {
        impls.append_all(quote! {
            impl #impl_generics ::std::fmt::Debug for #name #ty_generics #where_clause {
                fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                    ::style_traits::ToCss::to_css(
                        self,
                        &mut ::style_traits::CssWriter::new(f),
                    )
                }
            }
        });
    }

    impls
}

fn derive_variant_arm(
    variant: &VariantInfo,
    where_clause: &mut WhereClause,
) -> Tokens {
    let bindings = variant.bindings();
    let identifier = cg::to_css_identifier(variant.ast().ident.as_ref());
    let ast = variant.ast();
    let variant_attrs = cg::parse_variant_attrs::<CssVariantAttrs>(&ast);
    let separator = if variant_attrs.comma { ", " } else { " " };

    if variant_attrs.dimension {
        assert_eq!(bindings.len(), 1);
        assert!(
            variant_attrs.function.is_none() && variant_attrs.keyword.is_none(),
            "That makes no sense"
        );
    }

    let mut expr = if let Some(keyword) = variant_attrs.keyword {
        assert!(bindings.is_empty());
        let keyword = keyword.to_string();
        quote! {
            ::std::fmt::Write::write_str(dest, #keyword)
        }
    } else if !bindings.is_empty() {
        derive_variant_fields_expr(bindings, where_clause, separator)
    } else {
        quote! {
            ::std::fmt::Write::write_str(dest, #identifier)
        }
    };

    if variant_attrs.dimension {
        expr = quote! {
            #expr?;
            ::std::fmt::Write::write_str(dest, #identifier)
        }
    } else if let Some(function) = variant_attrs.function {
        let mut identifier = function.explicit().map_or(identifier, |name| name);
        identifier.push_str("(");
        expr = quote! {
            ::std::fmt::Write::write_str(dest, #identifier)?;
            #expr?;
            ::std::fmt::Write::write_str(dest, ")")
        }
    }
    expr
}

fn derive_variant_fields_expr(
    bindings: &[BindingInfo],
    where_clause: &mut WhereClause,
    separator: &str,
) -> Tokens {
    let mut iter = bindings.iter().filter_map(|binding| {
        let attrs = cg::parse_field_attrs::<CssFieldAttrs>(&binding.ast());
        if attrs.skip {
            return None;
        }
        Some((binding, attrs))
    }).peekable();

    let (first, attrs) = match iter.next() {
        Some(pair) => pair,
        None => return quote! { Ok(()) },
    };
    if !attrs.iterable && iter.peek().is_none() {
        if !attrs.ignore_bound {
            where_clause.add_trait_bound(&first.ast().ty);
        }
        return quote! { ::style_traits::ToCss::to_css(#first, dest) };
    }

    let mut expr = derive_single_field_expr(first, attrs, where_clause);
    for (binding, attrs) in iter {
        derive_single_field_expr(binding, attrs, where_clause).to_tokens(&mut expr)
    }

    quote! {{
        let mut writer = ::style_traits::values::SequenceWriter::new(dest, #separator);
        #expr
        Ok(())
    }}
}

fn derive_single_field_expr(
    field: &BindingInfo,
    attrs: CssFieldAttrs,
    where_clause: &mut WhereClause,
) -> Tokens {
    if attrs.iterable {
        if let Some(if_empty) = attrs.if_empty {
            return quote! {
                {
                    let mut iter = #field.iter().peekable();
                    if iter.peek().is_none() {
                        writer.item(&::style_traits::values::Verbatim(#if_empty))?;
                    } else {
                        for item in iter {
                            writer.item(&item)?;
                        }
                    }
                }
            };
        }
        quote! {
            for item in #field.iter() {
                writer.item(&item)?;
            }
        }
    } else {
        if !attrs.ignore_bound {
            where_clause.add_trait_bound(&field.ast().ty);
        }
        quote! { writer.item(#field)?; }
    }
}

#[darling(attributes(css), default)]
#[derive(Default, FromDeriveInput)]
struct CssInputAttrs {
    derive_debug: bool,
    // Here because structs variants are also their whole type definition.
    function: Option<Override<String>>,
    // Here because structs variants are also their whole type definition.
    comma: bool,
}

#[darling(attributes(css), default)]
#[derive(Default, FromVariant)]
pub struct CssVariantAttrs {
    pub function: Option<Override<String>>,
    pub comma: bool,
    pub dimension: bool,
    pub keyword: Option<String>,
    pub aliases: Option<String>,
}

#[darling(attributes(css), default)]
#[derive(Default, FromField)]
struct CssFieldAttrs {
    if_empty: Option<String>,
    ignore_bound: bool,
    iterable: bool,
    skip: bool,
}
