use crate::error::EngineError;
use crate::question::Question;
use crate::tokenizer::LayaTokenizer;

/// Token ids for one (state, question) pair and the positions of its option markers.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedQuestion {
    pub input_ids: Vec<u32>,
    pub markers: Vec<usize>,
    pub kind_index: usize,
}

/// Reproduces Laya's `build_sequence`:
/// `[CLS] <type> question: <instructions> [SEP] [MASK] option0 [MASK] option1 ... [SEP] state [SEP]`.
pub struct SequenceBuilder {
    pub max_len: usize,
    pub head_max_len: usize,
}

impl SequenceBuilder {
    const OPTION_TOKEN_LIMIT: usize = 48;
    const MINIMUM_HEAD_BUDGET: usize = 16;
    const MINIMUM_OPTION_TOKENS: usize = 4;
    const MINIMUM_INSTRUCTION_TOKENS: usize = 8;

    /// `state_ids` is the tokenized state, shared by every question asked about it.
    /// `truncate_left` keeps the end of a long state (Laya does this for JSON arrays such as chats).
    pub fn build(
        &self,
        tokenizer: &LayaTokenizer,
        question_id: &str,
        question: &Question,
        state_ids: &[u32],
        truncate_left: bool,
    ) -> Result<EncodedQuestion, EngineError> {
        let head_text = format!("{} question: {}", question.kind.name(), tokenizer.neutralize(&question.instructions));
        let mut head_ids = tokenizer.encode(&head_text)?;
        let mut option_ids = Vec::with_capacity(question.options.len());
        for option in &question.options {
            let mut tokens = tokenizer.encode(&format!(" {}", tokenizer.neutralize(&option.text)))?;
            tokens.truncate(Self::OPTION_TOKEN_LIMIT);
            let mut marked = Vec::with_capacity(tokens.len() + 1);
            marked.push(tokenizer.mask_id);
            marked.extend(tokens);
            option_ids.push(marked);
        }
        let options_length = |options: &[Vec<u32>]| options.iter().map(Vec::len).sum::<usize>();
        let mut option_budget = self.head_max_len as isize - options_length(&option_ids) as isize;
        if option_budget < Self::MINIMUM_HEAD_BUDGET as isize {
            let per_option = Self::MINIMUM_OPTION_TOKENS
                .max(self.head_max_len.saturating_sub(Self::MINIMUM_HEAD_BUDGET) / option_ids.len().max(1));
            for option in &mut option_ids {
                option.truncate(per_option);
            }
            option_budget = self.head_max_len as isize - options_length(&option_ids) as isize;
        }
        head_ids.truncate((Self::MINIMUM_INSTRUCTION_TOKENS as isize).max(option_budget) as usize);

        let mut input_ids = Vec::with_capacity(self.max_len);
        input_ids.push(tokenizer.cls_id);
        input_ids.extend(head_ids);
        input_ids.push(tokenizer.sep_id);
        let mut markers = Vec::with_capacity(option_ids.len());
        for option in option_ids {
            markers.push(input_ids.len());
            input_ids.extend(option);
        }
        input_ids.push(tokenizer.sep_id);
        let room = self.max_len.saturating_sub(input_ids.len() + 1);
        let kept_state = if truncate_left {
            &state_ids[state_ids.len().saturating_sub(room)..]
        } else {
            &state_ids[..state_ids.len().min(room)]
        };
        input_ids.extend_from_slice(kept_state);
        input_ids.push(tokenizer.sep_id);
        input_ids.truncate(self.max_len);
        markers.retain(|position| *position < self.max_len);
        if markers.len() != question.options.len() {
            return Err(EngineError::OptionsOverflow {
                id: question_id.to_string(),
                expected: question.options.len(),
                fitted: markers.len(),
                head_max_len: self.head_max_len,
            });
        }
        Ok(EncodedQuestion { input_ids, markers, kind_index: question.kind.index() })
    }
}
