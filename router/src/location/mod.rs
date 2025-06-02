#![allow(missing_docs)]

use any_spawner::Executor;
use core::fmt::Debug;
use js_sys::Reflect;
use leptos::server::ServerActionError;
use reactive_graph::{
    computed::Memo,
    owner::provide_context,
    signal::{ArcRwSignal, ReadSignal},
    traits::With,
};
use send_wrapper::SendWrapper;
use std::{borrow::Cow, future::Future, marker::PhantomData};
use tachys::dom::window;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Event, HtmlAnchorElement, MouseEvent};

mod history;
mod server;
use crate::params::ParamsMap;
pub use history::*;
pub use server::*;

pub(crate) const BASE: UrlContext<BrowserUrlContext, &str> =
    UrlContext::new("https://leptos.dev");

pub trait UrlContextType {
    fn produce_from_thin_air() -> Self;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BrowserUrlContext;

impl UrlContextType for BrowserUrlContext {
    fn produce_from_thin_air() -> Self {
        Self
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RouterUrlContext;

impl UrlContextType for RouterUrlContext {
    fn produce_from_thin_air() -> Self {
        Self
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UrlContext<C: UrlContextType, T>(T, PhantomData<C>);

impl<C: UrlContextType, T> UrlContext<C, T> {
    pub const fn new(value: T) -> Self {
        Self(value, PhantomData)
    }

    pub fn map<'a, Q>(
        &'a self,
        mut mapper: impl FnMut(&'a T) -> Q,
    ) -> UrlContext<C, Q> {
        UrlContext(mapper(&self.0), PhantomData)
    }

    pub fn map_mut<'a, Q>(
        &'a mut self,
        mut mapper: impl FnMut(&'a mut T) -> Q,
    ) -> UrlContext<C, Q> {
        UrlContext(mapper(&mut self.0), PhantomData)
    }

    pub fn forget_context(&self, _context: C) -> &T {
        &self.0
    }

    pub fn change_context<C2: UrlContextType>(
        self,
        _context: C,
    ) -> UrlContext<C2, T> {
        UrlContext(self.0, PhantomData)
    }
}

pub type RouterContext<T> = UrlContext<RouterUrlContext, T>;

pub type BrowserContext<T> = UrlContext<BrowserUrlContext, T>;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Url {
    origin: String,
    path: String,
    search: String,
    search_params: ParamsMap,
    hash: String,
}

impl<C: UrlContextType> UrlContext<C, Url> {
    pub fn origin(&self) -> UrlContext<C, &str> {
        self.map(|u| u.origin.as_str())
    }

    pub fn origin_mut(&mut self) -> UrlContext<C, &mut String> {
        self.map_mut(|u| &mut u.origin)
    }

    pub fn path(&self) -> UrlContext<C, &str> {
        self.map(|u| u.path.as_str())
    }

    pub fn path_mut(&mut self) -> UrlContext<C, &mut str> {
        self.map_mut(|u| u.path.as_mut_str())
    }

    pub fn search(&self) -> UrlContext<C, &str> {
        self.map(|u| u.search.as_str())
    }

    pub fn search_mut(&mut self) -> UrlContext<C, &mut String> {
        self.map_mut(|u| &mut u.search)
    }

    pub fn search_params(&self) -> UrlContext<C, &ParamsMap> {
        self.map(|u| &u.search_params)
    }

    pub fn search_params_mut(&mut self) -> UrlContext<C, &mut ParamsMap> {
        self.map_mut(|u| &mut u.search_params)
    }

    pub fn hash(&self) -> UrlContext<C, &str> {
        self.map(|u| u.hash.as_str())
    }

    pub fn hash_mut(&mut self) -> UrlContext<C, &mut String> {
        self.map_mut(|u| &mut u.hash)
    }

    pub fn provide_server_action_error(&self) {
        let search_params = self.search_params();
        if let (Some(err), Some(path)) = (
            search_params
                .forget_context(C::produce_from_thin_air())
                .get_str("__err"),
            search_params
                .forget_context(C::produce_from_thin_air())
                .get_str("__path"),
        ) {
            provide_context(ServerActionError::new(path, err))
        }
    }

    pub(crate) fn to_full_path(&self) -> UrlContext<C, String> {
        let mut path = self.map(|u| u.path.to_string());
        self.map(|u| {
            if !u.search.is_empty() {
                path.map_mut(|p| p.push('?'));
                path.map_mut(|p| p.push_str(&u.search));
            }
        });
        self.map(|u| {
            if !u.hash.is_empty() {
                if !u.hash.starts_with('#') {
                    path.map_mut(|p| p.push('#'));
                }
                path.map_mut(|p| p.push_str(&u.hash));
            }
        });
        path
    }

    pub fn escape(s: UrlContext<C, &str>) -> UrlContext<C, String> {
        #[cfg(not(feature = "ssr"))]
        {
            s.map(|s| js_sys::encode_uri_component(s).as_string().unwrap())
        }
        #[cfg(feature = "ssr")]
        {
            s.map(|s| {
                percent_encoding::utf8_percent_encode(
                    s,
                    percent_encoding::NON_ALPHANUMERIC,
                )
                .to_string()
            })
        }
    }

    pub fn unescape(s: UrlContext<C, &str>) -> UrlContext<C, String> {
        #[cfg(feature = "ssr")]
        {
            s.map(|s| {
                percent_encoding::percent_decode_str(s)
                    .decode_utf8()
                    .unwrap()
                    .to_string()
            })
        }

        #[cfg(not(feature = "ssr"))]
        {
            s.map(|s| match js_sys::decode_uri_component(s) {
                Ok(v) => v.into(),
                Err(_) => (*s).into(),
            })
        }
    }

    pub fn unescape_minimal(s: UrlContext<C, &str>) -> UrlContext<C, String> {
        #[cfg(not(feature = "ssr"))]
        {
            s.map(|s| match js_sys::decode_uri(s) {
                Ok(v) => v.into(),
                Err(_) => (*s).into(),
            })
        }

        #[cfg(feature = "ssr")]
        {
            s.map(|s| Self::unescape(s))
        }
    }
}

/// A reactive description of the current URL, containing equivalents to the local parts of
/// the browser's [`Location`](https://developer.mozilla.org/en-US/docs/Web/API/Location).
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// The path of the URL, not containing the query string or hash fragment.
    pub pathname: Memo<RouterContext<String>>,
    /// The raw query string.
    pub search: Memo<RouterContext<String>>,
    /// The query string parsed into its key-value pairs.
    pub query: Memo<RouterContext<ParamsMap>>,
    /// The hash fragment.
    pub hash: Memo<RouterContext<String>>,
    /// The [`state`](https://developer.mozilla.org/en-US/docs/Web/API/History/state) at the top of the history stack.
    pub state: ReadSignal<State>,
}

impl Location {
    pub(crate) fn new(
        url: impl Into<ReadSignal<RouterContext<Url>>>,
        state: impl Into<ReadSignal<State>>,
    ) -> Self {
        let url = url.into();
        let state = state.into();
        let pathname =
            Memo::new(move |_| url.with(|url| url.map(|url| url.path.clone())));
        let search = Memo::new(move |_| {
            url.with(|url| url.map(|url| url.search.clone()))
        });
        let hash =
            Memo::new(move |_| url.with(|url| url.map(|url| url.hash.clone())));
        let query = Memo::new(move |_| {
            url.with(|url| url.map(|url| url.search_params.clone()))
        });
        Location {
            pathname,
            search,
            query,
            hash,
            state,
        }
    }
}

/// A description of a navigation.
#[derive(Debug, Clone, PartialEq)]
pub struct LocationChange {
    /// The new URL.
    pub value: UrlContext<RouterUrlContext, std::string::String>,
    /// If true, the new location will replace the current one in the history stack, i.e.,
    /// clicking the "back" button will not return to the current location.
    pub replace: bool,
    /// If true, the router will scroll to the top of the page at the end of the navigation.
    pub scroll: bool,
    /// The [`state`](https://developer.mozilla.org/en-US/docs/Web/API/History/state) that will be added during navigation.
    pub state: State,
}

impl Default for LocationChange {
    fn default() -> Self {
        Self {
            value: Default::default(),
            replace: true,
            scroll: true,
            state: Default::default(),
        }
    }
}

pub trait LocationProvider: Clone + 'static {
    type Error: Debug;

    fn new() -> Result<Self, Self::Error>;

    fn as_url(&self) -> &ArcRwSignal<UrlContext<RouterUrlContext, Url>>;

    fn current() -> Result<UrlContext<RouterUrlContext, Url>, Self::Error>;

    /// Sets up any global event listeners or other initialization needed.
    fn init(&self, base: Option<Cow<'static, str>>);

    /// Should be called after a navigation when all route components and data have been loaded and
    /// the URL can be updated.
    fn ready_to_complete(&self);

    /// Update the browser's history to reflect a new location.
    fn complete_navigation(&self, loc: &LocationChange);

    fn parse(
        url: &str,
    ) -> Result<UrlContext<RouterUrlContext, Url>, Self::Error> {
        Self::parse_with_base(url, &BASE)
    }

    fn parse_with_base(
        url: &str,
        base: &UrlContext<BrowserUrlContext, &str>,
    ) -> Result<UrlContext<RouterUrlContext, Url>, Self::Error>;

    fn redirect(loc: &str);

    /// Whether we are currently in a "back" navigation.
    fn is_back(&self) -> ReadSignal<bool>;
}

#[derive(Debug, Clone, Default)]
pub struct State(Option<SendWrapper<JsValue>>);

impl State {
    pub fn new(state: Option<JsValue>) -> Self {
        Self(state.map(SendWrapper::new))
    }

    pub fn to_js_value(&self) -> JsValue {
        match &self.0 {
            Some(v) => v.clone().take(),
            None => JsValue::UNDEFINED,
        }
    }
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref().map(|n| n.as_ref())
            == other.0.as_ref().map(|n| n.as_ref())
    }
}

impl<T> From<T> for State
where
    T: Into<JsValue>,
{
    fn from(value: T) -> Self {
        State::new(Some(value.into()))
    }
}

pub(crate) fn handle_anchor_click<NavFn, NavFut>(
    router_base: Option<Cow<'static, str>>,
    parse_with_base: fn(
        &str,
        &UrlContext<BrowserUrlContext, &str>,
    )
        -> Result<UrlContext<RouterUrlContext, Url>, JsValue>,
    navigate: NavFn,
) -> Box<dyn Fn(Event) -> Result<(), JsValue>>
where
    NavFn: Fn(UrlContext<RouterUrlContext, Url>, LocationChange) -> NavFut
        + 'static,
    NavFut: Future<Output = ()> + 'static,
{
    let router_base = router_base.unwrap_or_default();

    Box::new(move |ev: Event| {
        let ev = ev.unchecked_into::<MouseEvent>();
        let origin = UrlContext::<BrowserUrlContext, _>::new(
            window().location().origin()?,
        );
        if ev.default_prevented()
            || ev.button() != 0
            || ev.meta_key()
            || ev.alt_key()
            || ev.ctrl_key()
            || ev.shift_key()
        {
            return Ok(());
        }

        let composed_path = ev.composed_path();
        let mut a: Option<HtmlAnchorElement> = None;
        for i in 0..composed_path.length() {
            if let Ok(el) = composed_path.get(i).dyn_into::<HtmlAnchorElement>()
            {
                a = Some(el);
            }
        }
        if let Some(a) = a {
            let href = a.href();
            let target = a.target();

            // let browser handle this event if link has target,
            // or if it doesn't have href or state
            // TODO "state" is set as a prop, not an attribute
            if !target.is_empty()
                || (href.is_empty() && !a.has_attribute("state"))
            {
                return Ok(());
            }

            let rel = a.get_attribute("rel").unwrap_or_default();
            let mut rel = rel.split([' ', '\t']);

            // let browser handle event if it has rel=external or download
            if a.has_attribute("download") || rel.any(|p| p == "external") {
                return Ok(());
            }

            let url = parse_with_base(
                href.as_str(),
                &origin.map(|origin| origin.as_str()),
            )
            .unwrap();
            let path_name =
                UrlContext::<RouterUrlContext, Url>::unescape_minimal(
                    url.path(),
                );

            // let browser handle this event if it leaves our domain
            // or our base path
            if url.origin()
                != origin.map(|o| o.as_str()).change_context(BrowserUrlContext)
                || (!router_base.is_empty()
                    && !path_name.forget_context(RouterUrlContext).is_empty()
                    // NOTE: the two `to_lowercase()` calls here added a total of about 14kb to
                    // release binary size, for limited gain
                    && !path_name.forget_context(RouterUrlContext).starts_with(&*router_base))
            {
                return Ok(());
            }

            // we've passed all the checks to navigate on the client side, so we prevent the
            // default behavior of the click
            ev.prevent_default();
            let to = path_name
                + if url.search().forget_context(RouterUrlContext).is_empty() {
                    ""
                } else {
                    "?"
                }
                + &UrlContext::<RouterUrlContext, Url>::unescape(url.search())
                + &UrlContext::<RouterUrlContext, Url>::unescape(url.hash());
            let state = Reflect::get(&a, &JsValue::from_str("state"))
                .ok()
                .and_then(|value| {
                    if value == JsValue::UNDEFINED {
                        None
                    } else {
                        Some(value)
                    }
                });

            let replace = Reflect::get(&a, &JsValue::from_str("replace"))
                .ok()
                .and_then(|value| value.as_bool())
                .unwrap_or(false);

            let change = LocationChange {
                value: to,
                replace,
                scroll: !a.has_attribute("noscroll")
                    && !a.has_attribute("data-noscroll"),
                state: State::new(state),
            };

            Executor::spawn_local(navigate(url, change));
        }

        Ok(())
    })
}
