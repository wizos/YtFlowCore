use std::borrow::Cow;
use std::collections::BTreeMap;

use percent_encoding::percent_decode_str;
use thiserror::Error;
use url::{Host, Url};

use ytflow::flow::{DestinationAddr, HostName};

use crate::proxy::protocol::{
    http::HttpProxy, shadowsocks::ShadowsocksProxy, socks5::Socks5Proxy, trojan::TrojanProxy,
};
use crate::proxy::Proxy;

pub static BASE64_ENGINE: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::general_purpose::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DecodeError {
    #[error("invalid URL")]
    InvalidUrl,
    #[error("invalid URL, UTF-8 or Base64 encoding")]
    InvalidEncoding,
    #[error(r#""{0}" is required, but is missing"#)]
    MissingInfo(&'static str),
    #[error(r#"unknown value for field "{0}"#)]
    UnknownValue(&'static str),
    #[error("unknown URL scheme")]
    UnknownScheme,
    #[error(r#"extra parameter "{0}""#)]
    ExtraParameters(String),
}

pub type DecodeResult<T> = Result<T, DecodeError>;

pub(super) type QueryMap<'a> = BTreeMap<Cow<'a, str>, Cow<'a, str>>;

pub fn decode_share_link(link: &str) -> Result<Proxy, DecodeError> {
    let url = url::Url::parse(link.trim()).map_err(|_| DecodeError::InvalidUrl)?;
    let mut queries = url.query_pairs().collect::<QueryMap>();

    let proxy = match url.scheme() {
        "ss" => ShadowsocksProxy::decode_share_link(&url, &mut queries)?,
        "trojan" => TrojanProxy::decode_share_link(&url, &mut queries)?,
        "http" | "https"
            if url
                .host_str()
                .filter(|h| h.eq_ignore_ascii_case("t.me"))
                .is_some() =>
        {
            return Err(DecodeError::UnknownScheme)
        }
        "http" | "https" => HttpProxy::decode_share_link(&url, &mut queries)?,
        "socks5" => Socks5Proxy::decode_share_link(&url, &mut queries)?,
        _ => return Err(DecodeError::UnknownScheme),
    };

    if let Some((first_extra_key, _)) = queries.pop_first() {
        return Err(DecodeError::ExtraParameters(first_extra_key.into()));
    }

    Ok(proxy)
}

pub(super) fn extract_name_from_frag(url: &Url, dest: &DestinationAddr) -> DecodeResult<String> {
    Ok(url
        .fragment()
        .map(|s| percent_decode_str(s).decode_utf8())
        .transpose()
        .map_err(|_| DecodeError::InvalidEncoding)?
        .map(|s| s.into_owned())
        .unwrap_or_else(|| dest.to_string()))
}

/// Parse the host part again without scheme information.
/// ss, trojan or vmess URLs are not ["special"](https://url.spec.whatwg.org/#is-special), hence IPv4 hosts are
/// treated as domain names. Parsing again is necessary to handle IPv4 hosts correctly.
pub(super) fn parse_host_transparent(url: &Url) -> DecodeResult<HostName> {
    match Host::parse(url.host_str().unwrap_or_default()).map_err(|_| DecodeError::InvalidEncoding)
    {
        Ok(host) => Ok(map_host_name(host)),
        Err(_) => Err(DecodeError::InvalidEncoding),
    }
}

pub(super) fn map_host_name(host: Host<impl Into<String>>) -> HostName {
    match host {
        Host::Domain(domain) => {
            HostName::from_domain_name(domain.into()).expect("a valid domain name")
        }
        Host::Ipv4(ip) => HostName::Ip(ip.into()),
        Host::Ipv6(ip) => HostName::Ip(ip.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;

    #[test]
    fn test_extract_name_from_frag() {
        let url = Url::parse("ss://test#cabc%2fabca").unwrap();
        let dest = DestinationAddr {
            host: ytflow::flow::HostName::from_domain_name("example.com".into()).unwrap(),
            port: 1234,
        };
        assert_eq!(
            extract_name_from_frag(&url, &dest).unwrap(),
            "cabc/abca".to_string()
        );
    }

    #[test]
    fn test_extract_name_from_frag_invalid() {
        let url = Url::parse("ss://test#cabc%ff%ffabca").unwrap();
        let dest = DestinationAddr {
            host: ytflow::flow::HostName::from_domain_name("example.com".into()).unwrap(),
            port: 1234,
        };
        assert_eq!(
            extract_name_from_frag(&url, &dest),
            Err(DecodeError::InvalidEncoding)
        );
    }

    #[test]
    fn test_parse_host_transparent() {
        let cases = [
            ("3.187.225.7", HostName::Ip([3, 187, 225, 7].into())),
            ("a.co", HostName::DomainName("a.co".into())),
            ("[::1]", HostName::Ip(Ipv6Addr::LOCALHOST.into())),
        ];
        for (host_part, expected_host) in cases {
            let host = Url::parse(&format!("ss://{}:1080", host_part)).unwrap();
            let host = parse_host_transparent(&host).unwrap();
            assert_eq!(host, expected_host);
        }
    }
    #[test]
    fn test_parse_host_transparent_invalid_encoding() {
        let url = Url::parse(&format!("ss://a%25b:34187")).unwrap();
        let host = parse_host_transparent(&url);
        assert_eq!(host.unwrap_err(), DecodeError::InvalidEncoding);
    }
    #[test]
    fn test_map_host_name() {
        let cases = [
            ("3.187.225.7", HostName::Ip([3, 187, 225, 7].into())),
            ("a.co", HostName::DomainName("a.co".into())),
            ("[::1]", HostName::Ip(Ipv6Addr::LOCALHOST.into())),
        ];
        for (host_part, expected_host) in cases {
            let host = Host::parse(host_part).unwrap();
            let host = map_host_name(host);
            assert_eq!(host, expected_host);
        }
    }
}
