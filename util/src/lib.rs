#[cfg(feature = "console")]
pub mod console;

#[cfg(feature = "url-search-params")]
pub mod url_search_params;

#[cfg(test)]
pub(crate) fn test_with<F, R>(func: F)
where
    F: FnOnce(rquickjs::Ctx) -> R,
{
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    ctx.with(func);
}