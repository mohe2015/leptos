use super::{handle_anchor_click, LocationChange, LocationProvider, Url};
use crate::{
    hooks::use_navigate,
    location::{
        BrowserUrlContext, RouterUrlContext, UrlContext, UrlContexty as _,
    },
    params::ParamsMap,
};
use core::fmt;
use futures::channel::oneshot;
use js_sys::{try_iter, Array, JsString};
use leptos::prelude::*;
use or_poisoned::OrPoisoned;
use reactive_graph::{
    signal::ArcRwSignal,
    traits::{ReadUntracked, Set},
};
use std::{
    borrow::Cow,
    boxed::Box,
    string::String,
    sync::{Arc, Mutex},
};
use tachys::dom::{document, window};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{Event, UrlSearchParams};

#[derive(Clone)]
pub struct BrowserRouter {
    url: ArcRwSignal<UrlContext<RouterUrlContext, Url>>,
    pub(crate) pending_navigation: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    pub(crate) path_stack:
        ArcStoredValue<Vec<UrlContext<RouterUrlContext, Url>>>,
    pub(crate) is_back: ArcRwSignal<bool>,
}

impl fmt::Debug for BrowserRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserUrl").finish_non_exhaustive()
    }
}

impl BrowserRouter {
    fn scroll_to_el(loc_scroll: bool) {
        if let Ok(hash) = window().location().hash() {
            if !hash.is_empty() {
                let hash = js_sys::decode_uri(&hash[1..])
                    .ok()
                    .and_then(|decoded| decoded.as_string())
                    .unwrap_or(hash);
                let el = document().get_element_by_id(&hash);
                if let Some(el) = el {
                    el.scroll_into_view();
                    return;
                }
            }
        }

        // scroll to top
        if loc_scroll {
            window().scroll_to_with_x_and_y(0.0, 0.0);
        }
    }
}

impl LocationProvider for BrowserRouter {
    type Error = JsValue;

    fn new() -> Result<Self, JsValue> {
        let url = ArcRwSignal::new(Self::current()?);
        let path_stack = ArcStoredValue::new(
            Self::current().map(|n| vec![n]).unwrap_or_default(),
        );
        Ok(Self {
            url,
            pending_navigation: Default::default(),
            path_stack,
            is_back: Default::default(),
        })
    }

    fn as_url(&self) -> &ArcRwSignal<UrlContext<RouterUrlContext, Url>> {
        &self.url
    }

    fn current() -> Result<UrlContext<RouterUrlContext, Url>, Self::Error> {
        let location = window().location();
        Ok(UrlContext::new(Url {
            origin: location.origin()?,
            path: location.pathname()?,
            search: location
                .search()?
                .strip_prefix('?')
                .map(String::from)
                .unwrap_or_default(),
            search_params: search_params_from_web_url(
                &UrlSearchParams::new_with_str(&location.search()?)?,
            )?,
            hash: location.hash()?,
        }))
    }

    fn parse(
        url: UrlContext<RouterUrlContext, &str>,
    ) -> Result<UrlContext<RouterUrlContext, Url>, Self::Error> {
        let base = UrlContext::<BrowserUrlContext, _>::new(
            window().location().origin()?,
        );
        Self::parse_with_base(url, base.as_ref().map(|base| base.as_str()))
    }

    fn parse_with_base(
        url: UrlContext<RouterUrlContext, &str>,
        base: UrlContext<BrowserUrlContext, &str>,
    ) -> Result<UrlContext<RouterUrlContext, Url>, Self::Error> {
        // TODO FIXME unwrap
        let location = base.as_ref().map(|base| {
            web_sys::Url::new_with_base(
                url.forget_context(RouterUrlContext),
                base,
            )
            .unwrap()
        });
        Ok(location
            .map(|location| {
                Url {
                    origin: location.origin(),
                    path: location.pathname(),
                    search: location
                        .search()
                        .strip_prefix('?')
                        .map(String::from)
                        .unwrap_or_default(),
                    search_params: search_params_from_web_url(
                        &location.search_params(),
                    )
                    .unwrap(), // TODO FIXME unwrap
                    hash: location.hash(),
                }
            })
            .change_context(BrowserUrlContext))
    }

    fn init(
        &self,
        base: UrlContext<RouterUrlContext, Option<Cow<'static, str>>>,
    ) {
        let window = window();
        let navigate = {
            let url = self.url.clone();
            let pending = Arc::clone(&self.pending_navigation);
            let this = self.clone();
            move |new_url: UrlContext<RouterUrlContext, Url>, loc| {
                let same_path = {
                    let curr = url.read_untracked();
                    curr.origin() == new_url.origin()
                        && curr.path() == new_url.path()
                };

                url.set(new_url.clone());
                if same_path {
                    this.complete_navigation(&loc);
                }
                let pending = Arc::clone(&pending);
                let (tx, rx) = oneshot::channel::<()>();
                if !same_path {
                    *pending.lock().or_poisoned() = Some(tx);
                }
                let url = url.clone();
                let this = this.clone();
                async move {
                    if !same_path {
                        // if it has been canceled, ignore
                        // otherwise, complete navigation -- i.e., set URL in address bar
                        if rx.await.is_ok() {
                            // only update the URL in the browser if this is still the current URL
                            // if we've navigated to another page in the meantime, don't update the
                            // browser URL
                            let curr = url.read_untracked();
                            if curr == new_url {
                                this.complete_navigation(&loc);
                            }
                        }
                    }
                }
            }
        };

        let handle_anchor_click =
            handle_anchor_click(base, Self::parse_with_base, navigate);
        let closure = Closure::wrap(Box::new(move |ev: Event| {
            if let Err(e) = handle_anchor_click(ev) {
                #[cfg(feature = "tracing")]
                tracing::error!("{e:?}");
                #[cfg(not(feature = "tracing"))]
                web_sys::console::error_1(&e);
            }
        }) as Box<dyn FnMut(Event)>)
        .into_js_value();
        window
            .add_event_listener_with_callback(
                "click",
                closure.as_ref().unchecked_ref(),
            )
            .expect(
                "couldn't add `click` listener to `window` to handle `<a>` \
                 clicks",
            );

        // handle popstate event (forward/back navigation)
        let cb = {
            let url = self.url.clone();
            let path_stack = self.path_stack.clone();
            let is_back = self.is_back.clone();
            move || match Self::current() {
                Ok(new_url) => {
                    let stack = path_stack.read_value();
                    let is_navigating_back = stack.len() == 1
                        || (stack.len() >= 2
                            && stack.get(stack.len() - 2) == Some(&new_url));

                    is_back.set(is_navigating_back);

                    url.set(new_url);
                }
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    tracing::error!("{e:?}");
                    #[cfg(not(feature = "tracing"))]
                    web_sys::console::error_1(&e);
                }
            }
        };
        let closure =
            Closure::wrap(Box::new(cb) as Box<dyn Fn()>).into_js_value();
        window
            .add_event_listener_with_callback(
                "popstate",
                closure.as_ref().unchecked_ref(),
            )
            .expect("couldn't add `popstate` listener to `window`");
    }

    fn ready_to_complete(&self) {
        if let Some(tx) = self.pending_navigation.lock().or_poisoned().take() {
            _ = tx.send(());
        }
    }

    fn complete_navigation(&self, loc: &LocationChange) {
        let history = window().history().unwrap();

        if loc.replace {
            history
                .replace_state_with_url(
                    &loc.state.to_js_value(),
                    "",
                    Some(loc.value.as_ref().forget_context(RouterUrlContext)),
                )
                .unwrap();
        } else {
            // push the "forward direction" marker
            let state = &loc.state.to_js_value();
            history
                .push_state_with_url(
                    state,
                    "",
                    Some(loc.value.as_ref().forget_context(RouterUrlContext)),
                )
                .unwrap();
        }

        // add this URL to the "path stack" for detecting back navigations, and
        // unset "navigating back" state
        if let Ok(url) = Self::current() {
            self.path_stack.write_value().push(url);
            self.is_back.set(false);
        }

        // scroll to el
        Self::scroll_to_el(loc.scroll);
    }

    fn redirect(loc: &UrlContext<RouterUrlContext, &str>) {
        let navigate = use_navigate();
        let Some(url) = resolve_redirect_url(loc) else {
            return; // resolve_redirect_url() already logs an error
        };
        let current_origin = UrlContext::<BrowserUrlContext, _>::new(
            location().origin().unwrap(),
        );
        if url.as_ref().map(|url| url.origin())
            == current_origin.change_context(BrowserUrlContext)
        {
            let navigate = navigate.clone();
            // delay by a tick here, so that the Action updates *before* the redirect
            let href = url.map(|url| url.href());
            request_animation_frame(move || {
                navigate(
                    href.as_ref()
                        .map(|href| href.as_str())
                        .forget_context(RouterUrlContext),
                    Default::default(),
                );
            });
            // Use set_href() if the conditions for client-side navigation were not satisfied
        } else if let Err(e) =
            location().set_href(&url.forget_context(RouterUrlContext).href())
        {
            leptos::logging::error!("Failed to redirect: {e:#?}");
        }
    }

    fn is_back(&self) -> ReadSignal<bool> {
        self.is_back.read_only().into()
    }
}

fn search_params_from_web_url(
    params: &web_sys::UrlSearchParams,
) -> Result<ParamsMap, JsValue> {
    try_iter(params)?
        .into_iter()
        .flatten()
        .map(|pair| {
            pair.and_then(|pair| {
                let row = pair.dyn_into::<Array>()?;
                Ok((
                    String::from(row.get(0).dyn_into::<JsString>()?),
                    String::from(row.get(1).dyn_into::<JsString>()?),
                ))
            })
        })
        .collect()
}

/// Resolves a redirect location to an (absolute) URL.
pub(crate) fn resolve_redirect_url(
    loc: &UrlContext<RouterUrlContext, &str>,
) -> Option<UrlContext<RouterUrlContext, web_sys::Url>> {
    let origin = match window().location().origin() {
        Ok(origin) => origin,
        Err(e) => {
            leptos::logging::error!("Failed to get origin: {:#?}", e);
            return None;
        }
    };

    // TODO: Use server function's URL as base instead.
    let base = origin;

    loc.as_ref()
        .map_opt(|loc| match web_sys::Url::new_with_base(loc, &base) {
            Ok(url) => Some(url),
            Err(e) => {
                leptos::logging::error!(
                    "Invalid redirect location: {}",
                    e.as_string().unwrap_or_default(),
                );
                None
            }
        })
}
