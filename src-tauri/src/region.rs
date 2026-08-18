/// Region detection for App Store compliance (Guideline 5).
///
/// ChatGPT/OpenAI features must be deactivated in the mainland China
/// storefront, so we read the system region from the current locale (e.g.
/// `zh_CN` or `zh-Hans-CN`). The trailing component after the last `_`/`-` is
/// the ISO 3166-1 alpha-2 region code, so `zh_CN` → `CN`.
pub fn is_china_region() -> bool {
    let locale = sys_locale::get_locale().unwrap_or_default();
    let region = locale
        .rsplit(|c| c == '_' || c == '-')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    region == "CN"
}
