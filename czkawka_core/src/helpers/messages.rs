//! Messages: Utility for collecting and printing messages, warnings, and errors.

/// Stores messages, warnings, and errors for reporting.
#[derive(Debug, Default, Clone)]
pub struct Messages {
    pub critical: Option<String>,
    /// Informational messages.
    pub messages: Vec<String>,
    /// Warning messages.
    pub warnings: Vec<String>,
    /// Error messages.
    pub errors: Vec<String>,
}

impl Messages {
    /// Creates a new, empty `Messages` struct.
    pub fn new() -> Self {
        Default::default()
    }

    /// Prints all messages, warnings, and errors to the provided writer.
    pub fn print_messages_to_writer<T: std::io::Write>(&self, writer: &mut T) -> std::io::Result<()> {
        let text = self.create_messages_text();
        writer.write_all(text.as_bytes())
    }

    /// Creates a formatted string containing all messages, warnings, and errors.
    pub fn create_messages_text(&self) -> String {
        let mut text = String::new();
        push_section(
            &mut text,
            "------------------------------CRITICAL ERROR---------------------------",
            self.critical.as_slice(),
            "--------------------------END OF CRITICAL ERROR------------------------",
        );
        push_section(
            &mut text,
            "--------------------------------ERRORS---------------------------------",
            &self.errors,
            "----------------------------END OF ERRORS------------------------------",
        );
        push_section(
            &mut text,
            "-------------------------------MESSAGES--------------------------------",
            &self.messages,
            "---------------------------END OF MESSAGES-----------------------------",
        );
        push_section(
            &mut text,
            "-------------------------------WARNINGS--------------------------------",
            &self.warnings,
            "---------------------------END OF WARNINGS-----------------------------",
        );
        text.trim().to_string()
    }

    /// Extends this `Messages` struct with another, appending all messages, warnings, and errors.
    pub fn extend_with_another_messages(&mut self, messages: Self) {
        let (messages, warnings, errors, critical) = (messages.messages, messages.warnings, messages.errors, messages.critical);
        self.messages.extend(messages);
        self.warnings.extend(warnings);
        self.errors.extend(errors);
        if critical.is_some() {
            self.critical = critical;
        }
    }
}

fn push_section(text: &mut String, header: &str, lines: &[String], footer: &str) {
    if lines.is_empty() {
        return;
    }
    for line in std::iter::once(header).chain(lines.iter().map(String::as_str)).chain(std::iter::once(footer)) {
        text.push_str(line);
        text.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_messages_constructors_and_text_formatting() {
        // Test new()
        let msg = Messages::new();
        assert!(msg.messages.is_empty());
        assert!(msg.warnings.is_empty());
        assert!(msg.errors.is_empty());
        assert_eq!(msg.create_messages_text(), "");

        let msg = Messages {
            errors: vec!["Error 1".to_string(), "Error 2".to_string()],
            ..Default::default()
        };
        let text = msg.create_messages_text();
        assert!(text.contains("ERRORS"));
        assert!(text.contains("Error 1"));

        let msg = Messages {
            warnings: vec!["Warning 1".to_string()],
            ..Default::default()
        };
        let text = msg.create_messages_text();
        assert!(text.contains("WARNINGS"));

        let msg = Messages {
            messages: vec!["Message 1".to_string()],
            ..Default::default()
        };
        let text = msg.create_messages_text();
        assert!(text.contains("MESSAGES"));

        // Test all types together
        let mut msg = Messages::new();
        msg.messages.push("Info".to_string());
        msg.warnings.push("Warn".to_string());
        msg.errors.push("Err".to_string());
        let text = msg.create_messages_text();
        assert!(text.contains("MESSAGES"));
        assert!(text.contains("Info"));
        assert!(text.contains("WARNINGS"));
        assert!(text.contains("Warn"));
        assert!(text.contains("ERRORS"));
        assert!(text.contains("Err"));
    }

    #[test]
    fn test_messages_text_exact_layout() {
        let msg = Messages {
            critical: Some("Crit".to_string()),
            errors: vec!["Err".to_string()],
            ..Default::default()
        };
        assert_eq!(
            msg.create_messages_text(),
            "------------------------------CRITICAL ERROR---------------------------\n\
             Crit\n\
             --------------------------END OF CRITICAL ERROR------------------------\n\
             --------------------------------ERRORS---------------------------------\n\
             Err\n\
             ----------------------------END OF ERRORS------------------------------"
        );
    }

    #[test]
    fn test_extend_and_writer() {
        // Test extend_with_another_messages()
        let mut msg1 = Messages::new();
        msg1.messages.push("Msg1".to_string());
        msg1.warnings.push("Warn1".to_string());
        msg1.errors.push("Err1".to_string());

        let mut msg2 = Messages::new();
        msg2.messages.push("Msg2".to_string());
        msg2.warnings.push("Warn2".to_string());
        msg2.errors.push("Err2".to_string());

        msg1.extend_with_another_messages(msg2);

        assert_eq!(msg1.messages.len(), 2);
        assert_eq!(msg1.warnings.len(), 2);
        assert_eq!(msg1.errors.len(), 2);
        assert!(msg1.messages.contains(&"Msg1".to_string()));
        assert!(msg1.messages.contains(&"Msg2".to_string()));

        // Test print_messages_to_writer()
        let mut buffer = Vec::new();
        let result = msg1.print_messages_to_writer(&mut buffer);
        result.unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Msg1"));
        assert!(output.contains("Warn2"));
        assert!(output.contains("Err1"));
    }
}
