pub(crate) struct TextProjection {
    stream: &'static str,
    pending_utf8: Vec<u8>,
    pending_cr: bool,
    warned_invalid_utf8: bool,
}

impl TextProjection {
    pub(crate) fn new(stream: &'static str) -> Self {
        Self {
            stream,
            pending_utf8: Vec::new(),
            pending_cr: false,
            warned_invalid_utf8: false,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending_utf8.extend_from_slice(bytes);
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending_utf8) {
                Ok(valid) => {
                    let valid = valid.to_owned();
                    self.pending_utf8.clear();
                    self.project_chars(&valid, &mut output);
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        let valid = String::from_utf8(self.pending_utf8[..valid_up_to].to_vec())
                            .expect("validated UTF-8 prefix");
                        self.pending_utf8.drain(..valid_up_to);
                        self.project_chars(&valid, &mut output);
                    }
                    let Some(error_length) = error.error_len() else {
                        break;
                    };
                    self.pending_utf8.drain(..error_length);
                    if !self.warned_invalid_utf8 {
                        self.warned_invalid_utf8 = true;
                        self.project_chars(
                            &format!(
                                "[KAT: invalid UTF-8 in Runtime {} was replaced]\n",
                                self.stream
                            ),
                            &mut output,
                        );
                    }
                    self.project_chars("�", &mut output);
                }
            }
        }
        output
    }

    pub(crate) fn finish(&mut self) -> String {
        let mut output = String::new();
        if !self.pending_utf8.is_empty() {
            self.pending_utf8.clear();
            if !self.warned_invalid_utf8 {
                self.warned_invalid_utf8 = true;
                self.project_chars(
                    &format!(
                        "[KAT: invalid UTF-8 in Runtime {} was replaced]\n",
                        self.stream
                    ),
                    &mut output,
                );
            }
            self.project_chars("�", &mut output);
        }
        if self.pending_cr {
            self.pending_cr = false;
            output.push('\n');
        }
        output
    }

    fn project_chars(&mut self, text: &str, output: &mut String) {
        for character in text.chars() {
            if self.pending_cr {
                self.pending_cr = false;
                output.push('\n');
                if character == '\n' {
                    continue;
                }
            }
            match character {
                '\r' => self.pending_cr = true,
                '\n' | '\t' => output.push(character),
                character if character.is_control() => {
                    output.push_str(&format!("\\u{{{:04X}}}", character as u32));
                }
                character => output.push(character),
            }
        }
    }
}

pub(crate) fn project_complete_text(text: &str) -> String {
    let mut stripper = strip_ansi::StripStream::new();
    let mut plain = Vec::with_capacity(text.len());
    stripper.push(text.as_bytes(), &mut plain);
    stripper.finish();
    let mut projection = TextProjection::new("diagnostic");
    let mut output = projection.push(&plain);
    output.push_str(&projection.finish());
    output
}

pub(crate) fn project_inline_text(text: &str) -> String {
    project_complete_text(text)
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}
