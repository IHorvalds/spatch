use std::iter::Peekable;
use std::io::{self, BufRead};
type PeekableLines<T> = Peekable<io::Lines<io::BufReader<T>>>;

pub struct DiffParser<T: io::Read> {
    lines: PeekableLines<T>,
}

impl<T> DiffParser<T>
where
    T: Sized + io::Read,
{
    pub fn new(handle: T) -> Self {
        DiffParser {
            lines: io::BufReader::new(handle).lines().peekable(),
        }
    }

    pub fn next_patch(&mut self) -> Option<Patch<'_, T>> {
        // Skip to the next "diff" line.
        let mut iter = self
            .lines
            .by_ref()
            .filter_map(|l| l.ok())
            .skip_while(|l| !l.starts_with("diff "));

        // Extract header, old and new filenames.
        let mut header = String::new();
        let a = iter.find(|line| {
            header.push_str(line.as_str());
            header.push('\n');
            line.starts_with("--- ")
        })?[4..]
            .replacen("a/", "", 1);

        let new = iter.next()?;
        header.push_str(new.as_str());
        header.push('\n');

        let b = new[4..].replacen("b/", "", 1);
        Some(Patch::new(a, b, header, self))
    }
}
pub struct Patch<'a, T: io::Read> {
    old_filename: String,
    new_filename: String,
    header: String,
    lines_left: u32,
    p: char,
    parser: &'a mut DiffParser<T>,
}

impl<'a, T> Patch<'a, T>
where
    T: io::Read,
{
    pub fn new(
        old_filename: String,
        new_filename: String,
        header: String,
        parser: &'a mut DiffParser<T>,
    ) -> Self {
        Patch {
            old_filename,
            new_filename,
            header,
            lines_left: 0,
            p: ' ',
            parser,
        }
    }

    pub fn old_filename(&self) -> &str {
        &self.old_filename
    }

    pub fn new_filename(&self) -> &str {
        &self.new_filename
    }

    pub fn header(&mut self) -> &str {
        &self.header
    }

    /// @@ -56,7 +56,8 @@ ...........
    ///       |^|   |^| that's what we want
    fn parse_hunk_start(line: &String) -> Option<(u32, u32)> {
        let (mut a, mut b) = line.strip_prefix("@@ -")?.split_once("+")?;
        a = a.split_once(",")?.1.trim();
        b = b.split_once(" @@")?.0.split_once(",")?.1;
        Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
    }

    pub fn lines(&'a mut self) -> PatchLines<'a, T> {
        PatchLines { patch: self }
    }
}

pub struct PatchLines<'a, T: io::Read> {
    patch: &'a mut Patch<'a, T>,
}

impl<'a, T> Iterator for PatchLines<'a, T>
where
    T: io::Read,
{
    type Item = String;
    fn next(&mut self) -> Option<Self::Item> {
        if self.patch.lines_left == 0 {
            let line = match self.patch.parser.lines.peek() {
                Some(Ok(line)) => line,
                _ => return None,
            };
            if let Some((a, b)) = Patch::<T>::parse_hunk_start(line) {
                if a > b {
                    self.patch.lines_left = a;
                    self.patch.p = '-';
                } else {
                    self.patch.lines_left = b;
                    self.patch.p = '+';
                }
                return Some(self.patch.parser.lines.next().unwrap().unwrap()); // Consume the hunk header.
            } else {
                return None;
            }
        }
        if let Some(line) = self.patch.parser.lines.next() {
            let line = match line {
                Ok(line) => line,
                Err(_) => return None,
            };
            if line.starts_with(self.patch.p) || line.starts_with(' ') {
                self.patch.lines_left -= 1;
            }
            Some(line)
        } else {
            None
        }
    }
}

