use reqwest::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    pub fn parse(s: &str) -> Option<Method> {
        Method::ALL
            .into_iter()
            .find(|m| m.as_str().eq_ignore_ascii_case(s))
    }

    pub(crate) fn to_reqwest(self) -> reqwest::Method {
        match self {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
            Method::Head => reqwest::Method::HEAD,
            Method::Options => reqwest::Method::OPTIONS,
        }
    }
}

/// Validates an expanded URL: absolute, `http`/`https`, with a host.
pub fn parse_http_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("enter a URL such as http://localhost:8080/".to_owned());
    }
    let url = Url::parse(raw).map_err(|e| match e {
        url::ParseError::RelativeUrlWithoutBase => {
            "add the scheme, e.g. http:// or https://".to_owned()
        }
        other => other.to_string(),
    })?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "scheme \"{other}\" is not supported; use http:// or https://"
            ));
        }
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("the host is missing".to_owned());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_scheme_and_host() {
        assert!(parse_http_url("  ").is_err());
        assert!(
            parse_http_url("localhost:8080/x")
                .unwrap_err()
                .contains("scheme")
        );
        assert!(parse_http_url("/relative").unwrap_err().contains("scheme"));
        assert!(
            parse_http_url("file:///etc/passwd")
                .unwrap_err()
                .contains("file")
        );
        // Shell metacharacters are just URL characters, never executed.
        let ok = parse_http_url("http://127.0.0.1:1/a?q=$(rm -rf ~)&x=`id`").unwrap();
        assert_eq!(ok.host_str(), Some("127.0.0.1"));
        assert_eq!(Method::parse("patch"), Some(Method::Patch));
        assert_eq!(Method::parse("TRACE"), None);
    }
}
