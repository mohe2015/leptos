use std::borrow::Cow;

use crate::location::{
    RouterUrlContext, UrlContext, UrlContextType, UrlContexty as _,
};

pub fn resolve_path<'a>(
    base: UrlContext<RouterUrlContext, &'a str>,
    path: UrlContext<RouterUrlContext, &'a str>,
    from: UrlContext<RouterUrlContext, Option<&'a str>>,
) -> UrlContext<RouterUrlContext, Cow<'a, str>> {
    if has_scheme(path) {
        path.map(|path| path.into())
    } else {
        let base_path = normalize(base, false);
        // map option inside
        let from_path =
            from.map_opt(|from| from).map(|from| normalize(from, false));
        let result = if let Some(from_path) = from_path {
            if path.test(|path| path.starts_with('/')) {
                base_path
            } else if (from_path.as_ref(), base_path.as_ref()).test(
                |(from_path, base_path)| {
                    from_path.find(base_path.as_ref()) != Some(0)
                },
            ) {
                (base_path, from_path)
                    .map(|(base_path, from_path)| base_path + from_path)
            } else {
                from_path
            }
        } else {
            base_path
        };

        let result_empty = result.as_ref().test(|result| result.is_empty());
        let prefix = if result_empty {
            UrlContext::new("/".into())
        } else {
            result
        };

        (prefix, normalize(path, result_empty)).map(|(prefix, c)| prefix + c)
    }
}

fn has_scheme(path: UrlContext<RouterUrlContext, &str>) -> bool {
    path.test(|path| {
        path.starts_with("//")
            || path.starts_with("tel:")
            || path.starts_with("mailto:")
            || path
                .split_once("://")
                .map(|(prefix, _)| {
                    prefix.chars().all(
                    |c: char| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9'),
                )
                })
                .unwrap_or(false)
    })
}

#[doc(hidden)]
fn normalize<C: UrlContextType>(
    path: UrlContext<C, &str>,
    omit_slash: bool,
) -> UrlContext<C, Cow<'_, str>> {
    let s = path.map(|p| p.trim_start_matches('/'));
    let trim_end = s.as_ref().map(|s| {
        s.chars()
            .rev()
            .take_while(|c| *c == '/')
            .count()
            .saturating_sub(1)
    });
    let s = s
        .map(|s| trim_end.map(|trim_end| &s[0..s.len() - trim_end]))
        .flatten();
    s.map(|s| {
        if s.is_empty() || omit_slash || begins_with_query_or_hash(s) {
            s.into()
        } else {
            format!("/{s}").into()
        }
    })
}

fn begins_with_query_or_hash(text: &str) -> bool {
    matches!(text.chars().next(), Some('#') | Some('?'))
}

/* TODO can remove?
#[doc(hidden)]
pub fn join_paths<'a>(from: &'a str, to: &'a str) -> String {
    let from = remove_wildcard(&normalize(from, false));
    from + normalize(to, false).as_ref()
}

fn remove_wildcard(text: &str) -> String {
    text.rsplit_once('*')
        .map(|(prefix, _)| prefix)
        .unwrap_or(text)
        .trim_end_matches('/')
        .to_string()
}
*/

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_query_string_with_opening_slash() {
        assert_eq!(normalize("/?foo=bar", false), "?foo=bar");
    }

    #[test]
    fn normalize_retain_trailing_slash() {
        assert_eq!(normalize("foo/bar/", false), "/foo/bar/");
    }

    #[test]
    fn normalize_dedup_trailing_slashes() {
        assert_eq!(normalize("foo/bar/////", false), "/foo/bar/");
    }
}
