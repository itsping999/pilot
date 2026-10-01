use crate::config::{ChatCompletionsProviderConfig, ClaudeMessagesProviderConfig};
use crate::models::RetrievedChunk;

pub trait Generator: Send + Sync {
    fn name(&self) -> &str;
    fn generate(&self, question: &str, context: &[RetrievedChunk])
    -> Result<String, GenerateError>;

    fn generate_stream(
        &self,
        question: &str,
        context: &[RetrievedChunk],
        on_token: &mut dyn FnMut(&str) -> Result<(), GenerateError>,
    ) -> Result<String, GenerateError> {
        let full = self.generate(question, context)?;
        on_token(&full)?;
        Ok(full)
    }
}

#[derive(Debug)]
pub enum GenerateError {
    Provider(String),
}

impl std::fmt::Display for GenerateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerateError::Provider(msg) => write!(f, "generator provider error: {msg}"),
        }
    }
}

impl std::error::Error for GenerateError {}

pub struct ExtractiveGenerator;

impl Generator for ExtractiveGenerator {
    fn name(&self) -> &str {
        "extractive"
    }

    fn generate(
        &self,
        question: &str,
        context: &[RetrievedChunk],
    ) -> Result<String, GenerateError> {
        if context.is_empty() {
            return Ok(if contains_cjk(question) {
                "未检索到足够的上下文，暂时无法回答。".to_string()
            } else {
                "I do not have enough retrieved context to answer.".to_string()
            });
        }

        if contains_cjk(question) {
            Ok(format_chinese_answer(question, context))
        } else {
            Ok(format_english_answer(question, context))
        }
    }

    fn generate_stream(
        &self,
        question: &str,
        context: &[RetrievedChunk],
        on_token: &mut dyn FnMut(&str) -> Result<(), GenerateError>,
    ) -> Result<String, GenerateError> {
        let full = self.generate(question, context)?;
        for line in full.split_inclusive('\n') {
            on_token(line)?;
        }
        Ok(full)
    }
}

fn contains_cjk(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

fn format_chinese_answer(question: &str, context: &[RetrievedChunk]) -> String {
    let mut answer = String::new();
    answer.push_str("根据检索到的资料，答案如下：\n\n");

    let excerpts = extract_relevant_excerpts(question, context, 5);

    for (index, excerpt) in excerpts.iter().enumerate() {
        answer.push_str(&format!("{}. {}\n", index + 1, excerpt));
    }

    answer
}

fn format_english_answer(question: &str, context: &[RetrievedChunk]) -> String {
    let mut answer = String::new();
    answer.push_str("Based on the retrieved context, the answer is:\n\n");

    let excerpts = extract_relevant_excerpts(question, context, 5);

    for (index, excerpt) in excerpts.iter().enumerate() {
        answer.push_str(&format!("{}. {}\n", index + 1, excerpt));
    }

    answer
}

fn extract_relevant_excerpts(
    question: &str,
    context: &[RetrievedChunk],
    limit: usize,
) -> Vec<String> {
    let terms = query_terms(question);
    let mut scored = Vec::new();
    let mut order = 0usize;

    for item in context {
        let units = text_units(&item.chunk.text);
        for (unit_idx, unit) in units.iter().enumerate() {
            for sentence in split_sentences(unit) {
                if sentence.len() < 10 || is_noise_line(&sentence) {
                    order += 1;
                    continue;
                }

                let score = excerpt_score(&sentence, &terms);
                if score > 0 || terms.is_empty() {
                    let is_list_header = sentence.ends_with('：') || sentence.ends_with(':');
                    let excerpt = if is_list_header {
                        let mut expanded = sentence.clone();
                        if unit.len() > sentence.len() {
                            // Same unit has more content after the header
                            expanded = unit.clone();
                        } else if unit_idx + 1 < units.len() {
                            // Header is alone in its unit — look ahead to
                            // the next unit for the actual list items.
                            let next = units[unit_idx + 1].trim();
                            if !next.is_empty() && !next.starts_with('#') {
                                expanded.push('\n');
                                expanded.push_str(next);
                            }
                        }
                        trim_sentence(&expanded, 400)
                    } else {
                        trim_sentence(&sentence, 220)
                    };
                    scored.push(ScoredExcerpt {
                        score,
                        order,
                        excerpt,
                    });
                }
                order += 1;
            }
        }
    }

    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.order.cmp(&right.order))
    });

    let mut excerpts = Vec::new();
    for item in scored {
        let excerpt = item.excerpt;
        if excerpt.chars().count() >= 15 && !excerpts.iter().any(|existing| existing == &excerpt) {
            excerpts.push(excerpt);
        }
        if excerpts.len() >= limit {
            break;
        }
    }

    if excerpts.is_empty()
        && let Some(first) = context.first()
    {
        excerpts.push(trim_sentence(&compact_text(&first.chunk.text), 220));
    }

    excerpts
}

fn is_noise_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return true;
    }
    if trimmed.starts_with("本文档")
        || trimmed.starts_with("本制度")
        || trimmed.starts_with("本规范")
        || trimmed.starts_with("本指引")
        || trimmed.starts_with("本办法")
        || trimmed.starts_with("本标准")
        || trimmed.starts_with("参考来源")
    {
        return true;
    }
    if trimmed.len() < 12
        && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
        && !trimmed.contains('：')
        && !trimmed.contains(':')
    {
        return true;
    }
    false
}

#[derive(Clone, Debug)]
struct ScoredExcerpt {
    score: usize,
    order: usize,
    excerpt: String,
}

fn excerpt_score(unit: &str, terms: &[String]) -> usize {
    let mut score = 0usize;
    for term in terms {
        if unit.contains(term.as_str()) {
            score += 1;
        }
    }
    score
}

fn query_terms(question: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii = String::new();
    let mut cjk = Vec::new();

    for ch in question.chars() {
        if ch.is_ascii_alphanumeric() {
            ascii.push(ch);
        } else {
            flush_ascii_query_term(&mut terms, &mut ascii);
            if is_cjk(ch) {
                cjk.push(ch);
            } else {
                flush_cjk_query_terms(&mut terms, &mut cjk);
            }
        }
    }

    flush_ascii_query_term(&mut terms, &mut ascii);
    flush_cjk_query_terms(&mut terms, &mut cjk);
    terms.sort();
    terms.dedup();
    terms
}

fn flush_ascii_query_term(terms: &mut Vec<String>, ascii: &mut String) {
    if ascii.len() >= 2 {
        terms.push(ascii.to_lowercase());
    }
    ascii.clear();
}

fn flush_cjk_query_terms(terms: &mut Vec<String>, cjk: &mut Vec<char>) {
    for width in 2..=3 {
        if cjk.len() < width {
            continue;
        }
        for window in cjk.windows(width) {
            terms.push(window.iter().collect());
        }
    }
    cjk.clear();
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{f900}'..='\u{faff}').contains(&ch)
}

fn text_units(text: &str) -> Vec<String> {
    text.split("\n\n").map(|s| s.to_string()).collect()
}

fn split_sentences(text: &str) -> Vec<String> {
    text.split(&['。', '；', '！', '？', '.', ';', '!', '?'][..])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn compact_cjk_spaces(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    for i in 0..chars.len() {
        if chars[i] == ' ' && i > 0 && i + 1 < chars.len() {
            let prev = chars[i - 1];
            let next = chars[i + 1];
            if prev.is_ascii_digit() && is_cjk(next) {
                // Only collapse when the digit is NOT part of a mixed
                // ASCII term like "P95", "RTO4".  Check up to 3 chars
                // before the digit for an ASCII letter.
                let has_adjacent_letter = (i >= 2 && chars[i - 2].is_ascii_alphabetic())
                    || (i >= 3
                        && chars[i - 2].is_ascii_digit()
                        && chars[i - 3].is_ascii_alphabetic());
                if !has_adjacent_letter {
                    continue;
                }
            }
            if is_cjk(prev) && next.is_ascii_digit() {
                // Collapse CJK -> digit space only when digit is not
                // followed by more digits then an ASCII letter (e.g. "第 3G 频段").
                continue;
            }
        }
        result.push(chars[i]);
    }
    result
}

fn trim_sentence(sentence: &str, max_len: usize) -> String {
    // Normalize newlines to spaces for clean display
    let normalized: String = sentence
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let normalized = normalized
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ");
    let cleaned = strip_section_prefix(&normalized);
    let cleaned = compact_cjk_spaces(&cleaned);
    if cleaned.len() <= max_len {
        return cleaned;
    }
    let trimmed: String = cleaned.chars().take(max_len).collect();
    format!("{trimmed}...")
}

fn strip_section_prefix(text: &str) -> String {
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        i += 1;
    }
    if i > 0 && i < chars.len() && chars[i] == ' ' {
        let heading_start = i + 1;
        let mut j = heading_start;
        while j < chars.len() && chars[j] != ' ' {
            j += 1;
        }
        // Count chars, not bytes
        if j - heading_start < 10 && j < chars.len() {
            let rest: String = chars[j..].iter().collect();
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

fn compact_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace() || *ch == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

pub struct ChatCompletionsGenerator {
    config: ChatCompletionsProviderConfig,
    client: reqwest::blocking::Client,
}

impl ChatCompletionsGenerator {
    pub fn new(config: ChatCompletionsProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl Generator for ChatCompletionsGenerator {
    fn name(&self) -> &str {
        "chat_completions"
    }

    fn generate(
        &self,
        question: &str,
        context: &[RetrievedChunk],
    ) -> Result<String, GenerateError> {
        let context_text = context
            .iter()
            .map(|c| c.chunk.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let system_msg = if contains_cjk(question) {
            "你是企业知识库问答助手。严格规则：1.只用参考资料原文回答，禁止编造；2.简洁中文，直接给要点；3.编号列表3-5条；4.资料不足回复\"根据当前资料无法确认\"；5.不提及参考资料等内部术语；6.不加总结或补充"
        } else {
            "You are an enterprise knowledge base assistant. Rules: 1.Only use provided context, never fabricate; 2.Concise professional answers; 3.Numbered lists, 3-5 points max; 4.If insufficient, reply: I do not have enough information; 5.Do not mention context/reference terms; 6.No summary statements"
        };

        let prompt = format!("参考资料：\n{context_text}\n\n问题：{question}");

        let mut request = self
            .client
            .post(self.config.endpoint_url())
            .json(&serde_json::json!({
                "model": self.config.model,
                "messages": [
                    {"role": "system", "content": system_msg},
                    {"role": "user", "content": prompt}
                ],
                "max_tokens": 1024,
                "temperature": 0.1
            }));

        if let Some(api_key) = &self.config.api_key {
            let header = &self.config.auth_header;
            let scheme = self.config.auth_scheme.as_deref().unwrap_or("Bearer");
            let value = format!("{scheme} {api_key}");
            request = request.header(header.as_str(), value.as_str());
        }

        for (name, value) in &self.config.extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = request
            .send()
            .map_err(|e| GenerateError::Provider(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|e| GenerateError::Provider(e.to_string()))?;

        if !status.is_success() {
            return Err(GenerateError::Provider(format!(
                "chat completions request failed with status {status}: {body}"
            )));
        }

        let response: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| GenerateError::Provider(e.to_string()))?;

        let answer = response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No answer generated.")
            .to_string();

        Ok(answer)
    }

    fn generate_stream(
        &self,
        question: &str,
        context: &[RetrievedChunk],
        on_token: &mut dyn FnMut(&str) -> Result<(), GenerateError>,
    ) -> Result<String, GenerateError> {
        use std::io::BufRead;

        let context_text = context
            .iter()
            .map(|c| c.chunk.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let system_msg = if contains_cjk(question) {
            "你是企业知识库问答助手。严格规则：1.只用参考资料原文回答，禁止编造；2.简洁中文，直接给要点；3.编号列表3-5条；4.资料不足回复\"根据当前资料无法确认\"；5.不提及参考资料等内部术语；6.不加总结或补充"
        } else {
            "You are an enterprise knowledge base assistant. Rules: 1.Only use provided context, never fabricate; 2.Concise professional answers; 3.Numbered lists, 3-5 points max; 4.If insufficient, reply: I do not have enough information; 5.Do not mention context/reference terms; 6.No summary statements"
        };

        let prompt = format!("参考资料：\n{context_text}\n\n问题：{question}");

        let mut request = self
            .client
            .post(self.config.endpoint_url())
            .json(&serde_json::json!({
                "model": self.config.model,
                "messages": [
                    {"role": "system", "content": system_msg},
                    {"role": "user", "content": prompt}
                ],
                "max_tokens": 1024,
                "temperature": 0.1,
                "stream": true
            }));

        if let Some(api_key) = &self.config.api_key {
            let header = &self.config.auth_header;
            let scheme = self.config.auth_scheme.as_deref().unwrap_or("Bearer");
            let value = format!("{scheme} {api_key}");
            request = request.header(header.as_str(), value.as_str());
        }

        for (name, value) in &self.config.extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = match request.send() {
            Ok(resp) if resp.status().is_success() => resp,
            _ => return self.generate(question, context),
        };

        let mut reader = std::io::BufReader::new(response);
        let mut line = String::new();
        let mut full_answer = String::new();

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }
            let trimmed = line.trim();
            if let Some(payload) = trimmed.strip_prefix("data: ") {
                let payload = payload.trim();
                if payload == "[DONE]" {
                    break;
                }
                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(payload)
                    && let Some(token) = chunk["choices"][0]["delta"]["content"].as_str()
                    && !token.is_empty()
                {
                    full_answer.push_str(token);
                    on_token(token)?;
                }
            }
            line.clear();
        }

        if full_answer.is_empty() {
            return self.generate(question, context);
        }

        Ok(full_answer)
    }
}

pub struct ClaudeMessagesGenerator {
    config: ClaudeMessagesProviderConfig,
    client: reqwest::blocking::Client,
}

impl ClaudeMessagesGenerator {
    pub fn new(config: ClaudeMessagesProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl Generator for ClaudeMessagesGenerator {
    fn name(&self) -> &str {
        "claude"
    }

    fn generate(
        &self,
        question: &str,
        context: &[RetrievedChunk],
    ) -> Result<String, GenerateError> {
        let context_text = context
            .iter()
            .map(|c| c.chunk.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let system_msg = if contains_cjk(question) {
            "你是企业知识库问答助手。严格规则：1.只用参考资料原文回答，禁止编造；2.简洁中文，直接给要点；3.编号列表3-5条；4.资料不足回复\"根据当前资料无法确认\"；5.不提及参考资料等内部术语；6.不加总结或补充"
        } else {
            "You are an enterprise knowledge base assistant. Rules: 1.Only use provided context, never fabricate; 2.Concise professional answers; 3.Numbered lists, 3-5 points max; 4.If insufficient, reply: I do not have enough information; 5.Do not mention context/reference terms; 6.No summary statements"
        };

        let prompt = format!("参考资料：\n{context_text}\n\n问题：{question}");

        let mut request = self
            .client
            .post(self.config.endpoint_url())
            .json(&serde_json::json!({
                "model": self.config.model,
                "max_tokens": self.config.max_tokens,
                "system": system_msg,
                "messages": [
                    {"role": "user", "content": prompt}
                ]
            }));

        if let Some(api_key) = &self.config.api_key {
            request = request.header("x-api-key", api_key.as_str());
            request = request.header("anthropic-version", self.config.anthropic_version.as_str());
        }

        for (name, value) in &self.config.extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = request
            .send()
            .map_err(|e| GenerateError::Provider(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|e| GenerateError::Provider(e.to_string()))?;

        if !status.is_success() {
            return Err(GenerateError::Provider(format!(
                "claude messages request failed with status {status}: {body}"
            )));
        }

        let response: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| GenerateError::Provider(e.to_string()))?;

        let answer = response["content"][0]["text"]
            .as_str()
            .unwrap_or("No answer generated.")
            .to_string();

        Ok(answer)
    }

    fn generate_stream(
        &self,
        question: &str,
        context: &[RetrievedChunk],
        on_token: &mut dyn FnMut(&str) -> Result<(), GenerateError>,
    ) -> Result<String, GenerateError> {
        use std::io::BufRead;

        let context_text = context
            .iter()
            .map(|c| c.chunk.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let system_msg = if contains_cjk(question) {
            "你是企业知识库问答助手。严格规则：1.只用参考资料原文回答，禁止编造；2.简洁中文，直接给要点；3.编号列表3-5条；4.资料不足回复\"根据当前资料无法确认\"；5.不提及参考资料等内部术语；6.不加总结或补充"
        } else {
            "You are an enterprise knowledge base assistant. Rules: 1.Only use provided context, never fabricate; 2.Concise professional answers; 3.Numbered lists, 3-5 points max; 4.If insufficient, reply: I do not have enough information; 5.Do not mention context/reference terms; 6.No summary statements"
        };

        let prompt = format!("参考资料：\n{context_text}\n\n问题：{question}");

        let mut request = self
            .client
            .post(self.config.endpoint_url())
            .json(&serde_json::json!({
                "model": self.config.model,
                "max_tokens": self.config.max_tokens,
                "system": system_msg,
                "messages": [
                    {"role": "user", "content": prompt}
                ],
                "stream": true
            }));

        if let Some(api_key) = &self.config.api_key {
            request = request.header("x-api-key", api_key.as_str());
            request = request.header("anthropic-version", self.config.anthropic_version.as_str());
        }

        for (name, value) in &self.config.extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = match request.send() {
            Ok(resp) if resp.status().is_success() => resp,
            _ => return self.generate(question, context),
        };

        let mut reader = std::io::BufReader::new(response);
        let mut line = String::new();
        let mut full_answer = String::new();

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }
            let trimmed = line.trim();
            if let Some(payload) = trimmed.strip_prefix("data: ") {
                let payload = payload.trim();
                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(payload)
                    && let Some(token) = chunk["delta"]["text"].as_str()
                    && !token.is_empty()
                {
                    full_answer.push_str(token);
                    on_token(token)?;
                }
            }
            line.clear();
        }

        if full_answer.is_empty() {
            return self.generate(question, context);
        }

        Ok(full_answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_section_prefix() {
        // Should strip "2 东西向" prefix
        let input = "2 东西向流量控制 传统安全架构侧重南北向流量防护";
        let result = strip_section_prefix(input);
        assert_eq!(result, "传统安全架构侧重南北向流量防护");

        // Should not strip non-section text
        let input2 = "零信任架构应建立全方位的安全态势感知能力";
        let result2 = strip_section_prefix(input2);
        assert_eq!(result2, input2);
    }

    #[test]
    fn test_trim_sentence_normalizes() {
        let input = "2 东西向流量控制\n传统安全架构\n侧重防护";
        let result = trim_sentence(input, 200);
        assert!(!result.contains('\n'));
        assert_eq!(result, "传统安全架构 侧重防护");
    }
}
