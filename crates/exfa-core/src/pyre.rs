//! Small backtracking regex for name search with Python `re.search(p, s, re.IGNORECASE)` semantics on the subset
//! Pyfa's search uses (jargon patterns, `re:` queries, `*` / `?` wildcards): literals and escapes, `.`, `\w \W \d \D
//! \s \S \b \B`, classes `[...]` (ranges, negation), groups `( )`, `(?: )`, look-ahead / look-behind `(?= ) (?! )
//! (?<= ) (?<! )`, alternation, greedy and lazy `* + ? {m} {m,} {m,n}`, anchors `^ $`. A pattern outside the subset
//! is an error (Pyfa treats a pattern that does not compile as "no match"). Always case-insensitive.

#[derive(Debug, Clone)]
enum Item {
    Ch(char),
    Range(char, char),
    Word(bool),
    Digit(bool),
    Space(bool),
}

#[derive(Debug, Clone)]
enum Node {
    Ch(char),
    Any,
    Class(Vec<Item>, bool),
    Word(bool),
    Digit(bool),
    Space(bool),
    Start,
    End,
    WordB(bool),
    Group(Vec<Vec<Node>>),
    Look { behind: bool, neg: bool, alt: Vec<Vec<Node>> },
    Rep { node: Box<Node>, min: usize, max: usize, greedy: bool },
}

pub struct Regex {
    alt: Vec<Vec<Node>>,
}

struct P<'a> {
    c: &'a [char],
    i: usize,
}

fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn alt(&mut self) -> Result<Vec<Vec<Node>>, String> {
        let mut alts = vec![self.seq()?];
        while self.eat('|') {
            alts.push(self.seq()?);
        }
        Ok(alts)
    }
    fn seq(&mut self) -> Result<Vec<Node>, String> {
        let mut s = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            s.push(self.quant(atom)?);
        }
        Ok(s)
    }
    fn num(&mut self) -> Option<usize> {
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.c[st..self.i].iter().collect::<String>().parse().ok()
    }
    fn quant(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => (0, usize::MAX),
            Some('+') => (1, usize::MAX),
            Some('?') => (0, 1),
            Some('{') => {
                let save = self.i;
                self.i += 1;
                let a = self.num();
                let r = if self.eat(',') {
                    let b = self.num();
                    if self.eat('}') { a.map(|a| (a, b.unwrap_or(usize::MAX))) } else { None }
                } else if self.eat('}') {
                    a.map(|a| (a, a))
                } else {
                    None
                };
                match r {
                    Some(r) => {
                        self.i -= 1; // re-consumed below
                        r
                    }
                    None => {
                        // not a quantifier: Python treats the brace literally
                        self.i = save;
                        return Ok(atom);
                    }
                }
            }
            _ => return Ok(atom),
        };
        self.i += 1;
        if matches!(atom, Node::Start | Node::End | Node::WordB(_) | Node::Look { .. }) {
            return Err("nothing to repeat".into());
        }
        let greedy = !self.eat('?');
        Ok(Node::Rep { node: Box::new(atom), min, max, greedy })
    }
    fn escape(&mut self) -> Result<Item, String> {
        let c = self.peek().ok_or("trailing backslash")?;
        self.i += 1;
        Ok(match c {
            'w' => Item::Word(true),
            'W' => Item::Word(false),
            'd' => Item::Digit(true),
            'D' => Item::Digit(false),
            's' => Item::Space(true),
            'S' => Item::Space(false),
            'n' => Item::Ch('\n'),
            't' => Item::Ch('\t'),
            'r' => Item::Ch('\r'),
            c if c.is_ascii_alphanumeric() => return Err(format!("unsupported escape \\{c}")),
            c => Item::Ch(c),
        })
    }
    fn atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end")?;
        self.i += 1;
        Ok(match c {
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '*' | '+' | '?' => return Err("nothing to repeat".into()),
            '\\' => match self.peek() {
                Some('b') => {
                    self.i += 1;
                    Node::WordB(true)
                }
                Some('B') => {
                    self.i += 1;
                    Node::WordB(false)
                }
                _ => match self.escape()? {
                    Item::Ch(c) => Node::Ch(fold(c)),
                    Item::Word(b) => Node::Word(b),
                    Item::Digit(b) => Node::Digit(b),
                    Item::Space(b) => Node::Space(b),
                    Item::Range(..) => unreachable!(),
                },
            },
            '[' => {
                let neg = self.eat('^');
                let mut items = Vec::new();
                let mut first = true;
                loop {
                    let c = self.peek().ok_or("unterminated character set")?;
                    if c == ']' && !first {
                        self.i += 1;
                        break;
                    }
                    first = false;
                    self.i += 1;
                    let it = if c == '\\' { self.escape()? } else { Item::Ch(c) };
                    if let Item::Ch(lo) = it {
                        if self.peek() == Some('-') && self.c.get(self.i + 1).is_some_and(|&n| n != ']') {
                            self.i += 1;
                            let hc = self.peek().unwrap();
                            self.i += 1;
                            let hi = if hc == '\\' {
                                match self.escape()? {
                                    Item::Ch(h) => h,
                                    _ => return Err("bad range".into()),
                                }
                            } else {
                                hc
                            };
                            if hi < lo {
                                return Err("bad character range".into());
                            }
                            items.push(Item::Range(lo, hi));
                            continue;
                        }
                    }
                    items.push(it);
                }
                Node::Class(items, neg)
            }
            '(' => {
                let node = if self.eat('?') {
                    if self.eat(':') {
                        Node::Group(self.alt()?)
                    } else if self.eat('=') {
                        Node::Look { behind: false, neg: false, alt: self.alt()? }
                    } else if self.eat('!') {
                        Node::Look { behind: false, neg: true, alt: self.alt()? }
                    } else if self.eat('<') {
                        let neg = if self.eat('!') {
                            true
                        } else if self.eat('=') {
                            false
                        } else {
                            return Err("unsupported group".into());
                        };
                        Node::Look { behind: true, neg, alt: self.alt()? }
                    } else {
                        return Err("unsupported group".into());
                    }
                } else {
                    Node::Group(self.alt()?)
                };
                if !self.eat(')') {
                    return Err("missing )".into());
                }
                node
            }
            ')' => return Err("unbalanced parenthesis".into()),
            c => Node::Ch(fold(c)),
        })
    }
}

fn item_match(it: &Item, c: char) -> bool {
    match *it {
        Item::Ch(x) => fold(x) == c,
        Item::Range(lo, hi) => {
            let (l, h) = (lo, hi);
            (l..=h).contains(&c) || c.to_uppercase().any(|u| (l..=h).contains(&u)) || (l..=h).contains(&fold(c))
        }
        Item::Word(b) => is_word(c) == b,
        Item::Digit(b) => c.is_numeric() == b,
        Item::Space(b) => c.is_whitespace() == b,
    }
}

struct M<'a> {
    s: &'a [char],
    /// lowercased haystack
    l: &'a [char],
    steps: std::cell::Cell<u32>,
}

impl M<'_> {
    fn one(&self, n: &Node, i: usize) -> bool {
        let Some(&c) = self.l.get(i) else { return false };
        match n {
            Node::Ch(x) => *x == c,
            Node::Any => c != '\n',
            Node::Class(items, neg) => items.iter().any(|it| item_match(it, c)) != *neg,
            Node::Word(b) => is_word(c) == *b,
            Node::Digit(b) => c.is_numeric() == *b,
            Node::Space(b) => c.is_whitespace() == *b,
            _ => false,
        }
    }
    fn alt(&self, alt: &[Vec<Node>], i: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
        alt.iter().any(|b| self.seq(b, i, k))
    }
    fn seq(&self, s: &[Node], i: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
        self.steps.set(self.steps.get().saturating_add(1));
        if self.steps.get() > 2_000_000 {
            return false;
        }
        let Some(n) = s.first() else { return k(i) };
        let rest = &s[1..];
        match n {
            Node::Start => i == 0 && self.seq(rest, i, k),
            Node::End => (i == self.s.len() || (i + 1 == self.s.len() && self.s[i] == '\n')) && self.seq(rest, i, k),
            Node::WordB(b) => {
                let before = i > 0 && is_word(self.s[i - 1]);
                let after = i < self.s.len() && is_word(self.s[i]);
                ((before != after) == *b) && self.seq(rest, i, k)
            }
            Node::Group(alt) => self.alt(alt, i, &mut |j| self.seq(rest, j, k)),
            Node::Look { behind, neg, alt } => {
                let hit = if *behind {
                    (0..=i).rev().any(|st| self.alt(alt, st, &mut |e| e == i))
                } else {
                    self.alt(alt, i, &mut |_| true)
                };
                hit != *neg && self.seq(rest, i, k)
            }
            Node::Rep { node, min, max, greedy } => self.rep(node, *min, *max, *greedy, 0, i, rest, k),
            _ => self.one(n, i) && self.seq(rest, i + 1, k),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn rep(&self, node: &Node, min: usize, max: usize, greedy: bool, count: usize, i: usize, rest: &[Node], k: &mut dyn FnMut(usize) -> bool) -> bool {
        let more = |this: &Self, k: &mut dyn FnMut(usize) -> bool| {
            count < max
                && this.seq(std::slice::from_ref(node), i, &mut |j| (j > i || count < min) && this.rep(node, min, max, greedy, count + 1, j, rest, k))
        };
        if count < min {
            return more(self, k);
        }
        if greedy {
            more(self, k) || self.seq(rest, i, k)
        } else {
            self.seq(rest, i, k) || more(self, k)
        }
    }
}

impl Regex {
    pub fn new(p: &str) -> Result<Regex, String> {
        let c: Vec<char> = p.chars().collect();
        let mut ps = P { c: &c, i: 0 };
        let alt = ps.alt()?;
        if ps.i != c.len() {
            return Err("unbalanced parenthesis".into());
        }
        Ok(Regex { alt })
    }
    /// `re.search(pattern, s, re.IGNORECASE) is not None`
    pub fn search(&self, s: &str) -> bool {
        let sc: Vec<char> = s.chars().collect();
        let lc: Vec<char> = sc.iter().map(|&c| fold(c)).collect();
        let m = M { s: &sc, l: &lc, steps: std::cell::Cell::new(0) };
        (0..=sc.len()).any(|st| m.alt(&self.alt, st, &mut |_| true))
    }
}

#[cfg(test)]
mod tests {
    use super::Regex;
    fn s(p: &str, t: &str) -> bool {
        Regex::new(p).unwrap().search(t)
    }
    #[test]
    fn python_like() {
        assert!(s("damage", "Damage Control II"));
        assert!(s("^Small Focused", "Small Focused Beam Laser II"));
        assert!(!s("^Focused", "Small Focused Beam Laser II"));
        assert!(s("(1| I$)", "Damage Control I"));
        assert!(!s("( I$)", "Damage Control II"));
        assert!(s("ancillary (.+ )?(?<!remote )armor repairer", "Small Ancillary Armor Repairer"));
        assert!(!s("(?<!remote )armor repairer", "Small Remote Armor Repairer I"));
        assert!(s("(^| )ab", "x ABC"));
        assert!(!s("(^| )ab", "1MN Afterburner"));
        assert!(s("10\\w*mn", "10MN Afterburner II"));
        assert!(s("[a-c]x{2,3}", "zBxxx"));
        assert!(s("dam\\w?ge", "damage"));
        assert!(s("warp disrupt(ion)? (.+ )?projector", "Warp Disruption Field Projector"));
        assert!(Regex::new("(abc").is_err());
        assert!(Regex::new("*a").is_err());
    }
}
