use jieba_rs::Jieba;
use std::collections::HashMap;
use std::sync::OnceLock;

static JIEBA: OnceLock<Jieba> = OnceLock::new();

const STOPWORDS: &[&str] = &[
    "的", "了", "是", "我", "你", "他", "她", "它", "们", "在", "也", "和", "就", "都", "而",
    "及", "与", "这", "那", "之", "其", "或", "一个", "一些", "可以", "我们", "你们", "他们",
    "什么", "怎么", "为什么", "如何", "如果", "因为", "所以", "但是", "不过", "这个", "那个",
    "the", "a", "an", "is", "are", "was", "were", "be", "to", "of", "in", "on", "and", "or",
    "for", "with", "this", "that", "it", "as", "at", "by", "from", "i", "you", "we", "they",
];

fn jieba() -> &'static Jieba {
    JIEBA.get_or_init(Jieba::new)
}

/// Extract up to `limit` representative keywords from `text` for auto-tagging suggestions.
pub fn extract_keywords(text: &str, limit: usize) -> Vec<String> {
    let words = jieba().cut(text, false);
    let mut freq: HashMap<String, usize> = HashMap::new();
    for w in words {
        let w = w.trim();
        let lower = w.to_lowercase();
        if w.chars().count() < 2 {
            continue;
        }
        if STOPWORDS.contains(&lower.as_str()) {
            continue;
        }
        if !w.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        *freq.entry(w.to_string()).or_insert(0) += 1;
    }
    let mut entries: Vec<(String, usize)> = freq.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    entries.into_iter().take(limit).map(|(w, _)| w).collect()
}
