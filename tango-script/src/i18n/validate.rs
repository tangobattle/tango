//! Keep Fluent's native number/plural operations and AST nesting bounded.
use fluent_bundle::FluentResource;
use fluent_syntax::ast;

use crate::{invalid, Result};

pub(super) fn number(value: f64) -> Result<()> {
    if !value.is_finite() || value.abs() > 9_007_199_254_740_991.0 {
        return Err(invalid("translation numbers must be finite and within ±(2^53−1)"));
    }
    Ok(())
}

fn literal(value: &str) -> Result<()> {
    if value.len() > 64 || value.split_once('.').is_some_and(|(_, fraction)| fraction.len() > 18) {
        return Err(invalid("translation number literals exceed supported precision"));
    }
    number(value.parse().map_err(|_| invalid("invalid translation number"))?)
}

pub(super) fn resource(resource: &FluentResource) -> Result<()> {
    for entry in resource.entries() {
        let attributes = match entry {
            ast::Entry::Message(message) => {
                if let Some(value) = &message.value {
                    pattern(value, 0)?;
                }
                &message.attributes
            }
            ast::Entry::Term(term) => {
                pattern(&term.value, 0)?;
                &term.attributes
            }
            _ => continue,
        };
        let mut names = std::collections::BTreeSet::new();
        for attribute in attributes {
            if !names.insert(attribute.id.name) {
                return Err(invalid("duplicate translation attribute"));
            }
            pattern(&attribute.value, 0)?;
        }
    }
    Ok(())
}

fn pattern(value: &ast::Pattern<&str>, depth: usize) -> Result<()> {
    for element in &value.elements {
        if let ast::PatternElement::Placeable { expression: value } = element {
            expression(value, depth + 1)?;
        }
    }
    Ok(())
}

fn expression(value: &ast::Expression<&str>, depth: usize) -> Result<()> {
    if depth > 32 {
        return Err(invalid("translation expressions exceed 32 levels"));
    }
    match value {
        ast::Expression::Inline(value) => inline(value, depth + 1)?,
        ast::Expression::Select { selector, variants } => {
            inline(selector, depth + 1)?;
            for variant in variants {
                if let ast::VariantKey::NumberLiteral { value } = &variant.key {
                    literal(value)?;
                }
                pattern(&variant.value, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn inline(value: &ast::InlineExpression<&str>, depth: usize) -> Result<()> {
    if depth > 32 {
        return Err(invalid("translation expressions exceed 32 levels"));
    }
    let arguments = match value {
        ast::InlineExpression::NumberLiteral { value } => return literal(value),
        ast::InlineExpression::Placeable { expression: value } => return expression(value, depth + 1),
        ast::InlineExpression::FunctionReference { id, arguments } => {
            if id.name == "NUMBER" {
                for argument in &arguments.named {
                    if argument.name.name.ends_with("Digits") {
                        let valid = match &argument.value {
                            ast::InlineExpression::NumberLiteral { value } => value
                                .parse::<f64>()
                                .is_ok_and(|n| (0.0..=18.0).contains(&n) && n.fract() == 0.0),
                            _ => false,
                        };
                        if !valid {
                            return Err(invalid("NUMBER precision options must be integers from 0 to 18"));
                        }
                    }
                }
            }
            arguments
        }
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => arguments,
        _ => return Ok(()),
    };
    for value in &arguments.positional {
        inline(value, depth + 1)?;
    }
    for argument in &arguments.named {
        inline(&argument.value, depth + 1)?;
    }
    Ok(())
}
