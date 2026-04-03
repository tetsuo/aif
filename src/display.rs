use super::ast::{BinaryOp, Expr, UnaryOp};
use std::fmt;

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.print(f, 0)
    }
}

impl Expr {
    fn print(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        match self {
            Expr::Binary { op, left, right, .. } => {
                let label = match op {
                    BinaryOp::And => "conjunction",
                    BinaryOp::Or => "disjunction",
                };
                writeln!(f, "{:indent$}{}", "", label, indent = indent)?;
                left.print(f, indent + 2)?;
                right.print(f, indent + 2)
            }

            Expr::Unary { op, expr, .. } => {
                let label = match op {
                    UnaryOp::Minus => "minus",
                    UnaryOp::Not => "not",
                };
                writeln!(f, "{:indent$}{}", "", label, indent = indent)?;
                expr.print(f, indent + 2)
            }

            Expr::Comparison { op, left, right, .. } => {
                writeln!(f, "{:indent$}compare {}", "", op.as_str(), indent = indent)?;
                left.print(f, indent + 2)?;
                right.print(f, indent + 2)
            }

            Expr::Name { name, is_string, .. } => {
                if *is_string {
                    writeln!(f, "{:indent$}{:?}", "", name, indent = indent)
                } else {
                    writeln!(f, "{:indent$}{}", "", name, indent = indent)
                }
            }

            Expr::Member { holder, member, .. } => {
                // Walk the member chain to collect all name segments.
                let mut parts = vec![member.as_str()];
                let mut cur = holder.as_ref();
                loop {
                    match cur {
                        Expr::Member { holder, member, .. } => {
                            parts.push(member.as_str());
                            cur = holder.as_ref();
                        }
                        Expr::Name { name, .. } => {
                            parts.push(name.as_str());
                            break;
                        }
                        _ => break,
                    }
                }
                parts.reverse();
                write!(f, "{:indent$}", "", indent = indent)?;
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ".")?;
                    }
                    write!(f, "{}", part)?;
                }
                writeln!(f)
            }

            Expr::Function { func, args } => {
                // "call" runs directly into the function name (no newline between them).
                // The function name is indented at indent+2, which produces the
                // two-space gap: "call  funcname\n".
                write!(f, "{:indent$}call", "", indent = indent)?;
                func.print(f, indent + 2)?;
                for arg in args {
                    arg.print(f, indent + 2)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::parse;

    #[test]
    fn test_display_comparison() {
        let expr = parse("x = 42").unwrap().unwrap();
        assert_eq!(format!("{expr}"), "compare =\n  x\n  42\n");
    }

    #[test]
    fn test_display_conjunction() {
        let expr = parse("a : b c = 17").unwrap().unwrap();
        assert_eq!(
            format!("{expr}"),
            "conjunction\n  compare :\n    a\n    b\n  compare =\n    c\n    17\n"
        );
    }

    #[test]
    fn test_display_member() {
        let expr = parse("a.b > 17").unwrap().unwrap();
        assert_eq!(format!("{expr}"), "compare >\n  a.b\n  17\n");
    }

    #[test]
    fn test_display_function() {
        let expr = parse("func(val)").unwrap().unwrap();
        assert_eq!(format!("{expr}"), "call  func\n  val\n");
    }

    #[test]
    fn test_display_string_literal() {
        // String literals are displayed with Rust's {:?} quoting.
        let expr = parse(r#"x = "hello""#).unwrap().unwrap();
        assert_eq!(format!("{expr}"), "compare =\n  x\n  \"hello\"\n");
    }
}
