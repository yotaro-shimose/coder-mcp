pub fn make_numbered_output(content: &str, start_line: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let numbered_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:6}\t{}", i + start_line, line))
        .collect();

    numbered_lines.join("\n")
}
