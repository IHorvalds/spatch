use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, Lines, Read};
use std::iter::Peekable;
use std::rc::Rc;

const GIT_DIFF_PREFIX: &'static str = "diff --git ";

type PeekableLines<T> = Rc<RefCell<Peekable<Lines<BufReader<T>>>>>;

pub struct DiffParser<T: Sized + Read> {
    lines: PeekableLines<T>,
}

impl<T> DiffParser<T>
where
    T: Sized + Read,
{
    pub fn new(handle: T) -> Self {
        DiffParser {
            lines: Rc::new(RefCell::new(BufReader::new(handle).lines().peekable())),
        }
    }

    fn next_patch(&mut self) -> Option<Patch<T>> {
        let mut lines_iter = self.lines.borrow_mut();
        // Skip to the next "diff" line.
        let mut iter = lines_iter
            .by_ref()
            .filter_map(|l| l.ok())
            .skip_while(|l| !l.starts_with(GIT_DIFF_PREFIX));

        // Extract header, old and new filenames.
        let mut header = iter.next()?;
        let mut old_filename;
        let mut new_filename;
        match header.strip_prefix(GIT_DIFF_PREFIX)?.split_once(" ") {
            Some((a, b)) => {
                old_filename = Self::filename(&a.to_string().replacen("a/", "", 1));
                new_filename = Self::filename(&b.to_string().replacen("b/", "", 1));
            }
            None => return None,
        };

        header += "\n";

        while let Some(Ok(line)) = lines_iter.next_if(Self::should_break) {
            if line.starts_with("--- ") {
                old_filename = Self::filename(&line[4..].replacen("a/", "", 1));
            } else if line.starts_with("+++ ") {
                new_filename = Self::filename(&line[4..].replacen("b/", "", 1));
            } else if let Some((a, b)) = line
                .strip_prefix("Binary files ")
                .and_then(|s| s.strip_suffix(" differ"))
                .and_then(|s| s.split_once(" and "))
            {
                old_filename = Self::filename(&a.replacen("a/", "", 1));
                new_filename = Self::filename(&b.replacen("b/", "", 1));
            }

            header.push_str(line.as_str());
            header.push('\n');
        }

        drop(lines_iter);

        Some(Patch::new(
            old_filename,
            new_filename,
            header,
            Rc::new(RefCell::new(self.clone())),
        ))
    }

    fn should_break(line: &Result<String, io::Error>) -> bool {
        match line {
            Ok(l) => !(l.starts_with(GIT_DIFF_PREFIX) || l.starts_with("@@ -")),
            _ => false,
        }
    }

    fn filename(f: &String) -> Option<String> {
        if f != "/dev/null" {
            let p = f.trim();
            let p1 = match p.strip_prefix("../") {
                Some(path) => path,
                None => p,
            };
            Some(
                match p1.strip_prefix("./") {
                    Some(path) => path,
                    None => p1,
                }
                .to_string(),
            )
        } else {
            None
        }
    }
}

impl<T> Clone for DiffParser<T>
where
    T: Sized + Read,
{
    fn clone(&self) -> Self {
        Self {
            lines: self.lines.clone(),
        }
    }
}

impl<T> Iterator for DiffParser<T>
where
    T: Sized + Read,
{
    type Item = Patch<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_patch()
    }
}

pub struct Patch<T: Sized + Read> {
    old_filename: Option<String>,
    new_filename: Option<String>,
    header: String,
    lines_left: u32,
    p: char,
    parser: Rc<RefCell<DiffParser<T>>>,
}

impl<T> Patch<T>
where
    T: Sized + Read,
{
    pub fn new(
        old_filename: Option<String>,
        new_filename: Option<String>,
        header: String,
        parser: Rc<RefCell<DiffParser<T>>>,
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

    pub fn old_filename(&self) -> &Option<String> {
        &self.old_filename
    }

    pub fn new_filename(&self) -> &Option<String> {
        &self.new_filename
    }

    pub fn header(&self) -> &str {
        &self.header
    }

    /// @@ -56,7 +56,8 @@ ...........
    ///       |^|   |^| that's what we want
    fn parse_hunk_start(line: &String) -> Option<(u32, u32)> {
        let (mut a, mut b) = line.strip_prefix("@@ -")?.split_once("+")?;
        a = a.trim();
        b = b.trim().split_once(" @@")?.0;

        Some((
            match a.split_once(",") {
                Some((_, suff)) => suff,
                None => a,
            }
            .parse::<u32>()
            .ok()?,
            match b.split_once(",") {
                Some((_, suff)) => suff,
                None => b,
            }
            .parse::<u32>()
            .ok()?,
        ))
    }

    pub fn lines<'a>(&mut self) -> PatchLines<'_, T> {
        PatchLines { patch: self }
    }
}

pub struct PatchLines<'a, T: Sized + Read> {
    patch: &'a mut Patch<T>,
}

impl<'a, T> Iterator for PatchLines<'a, T>
where
    T: Sized + Read,
{
    type Item = String;
    fn next(&mut self) -> Option<Self::Item> {
        let parser = self.patch.parser.borrow();
        let mut lines_iter = parser.lines.borrow_mut();
        if self.patch.lines_left == 0 {
            let line = match lines_iter.peek() {
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
                return Some(lines_iter.next().unwrap().unwrap()); // Consume the hunk header.
            } else {
                return None;
            }
        }
        if let Some(line) = lines_iter.next() {
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
