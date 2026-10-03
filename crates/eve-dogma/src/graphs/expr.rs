//! Tiny expression language for graph specs: numbers, names (`x`, `p.name`, `ship.attrName`, `stat.a.b`, kernel
//! results), `+ - * / ^`, unary minus, comparisons (`< <= > >= == !=`, 1/0), `cond ? a : b`, and calls
//! `f(a, b, ...)` (math built-ins or registered kernels). Parsed once into a tree, evaluated per sample point.

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

pub trait Env {
    /// value of a name; `None` = null (propagates)
    fn var(&mut self, name: &str) -> Result<Option<f64>, String>;
    /// kernel call (args already evaluated); `Err` if unknown
    fn call(&mut self, name: &str, args: &[Option<f64>]) -> Result<Option<f64>, String>;
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(t.as_bytes()) {
            self.i += t.len();
            true
        } else {
            false
        }
    }
    fn cond(&mut self) -> Result<Expr, String> {
        let c = self.or()?;
        if self.eat("?") {
            let a = self.cond()?;
            if !self.eat(":") {
                return Err("expected ':'".into());
            }
            let b = self.cond()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut a = self.and()?;
        while self.eat("||") {
            let b = self.and()?;
            a = Expr::Call("or".into(), vec![a, b]);
        }
        Ok(a)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut a = self.cmp()?;
        while self.eat("&&") {
            let b = self.cmp()?;
            a = Expr::Call("and".into(), vec![a, b]);
        }
        Ok(a)
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let a = self.add()?;
        for op in ["<=", ">=", "==", "!=", "<", ">"] {
            if self.eat(op) {
                let b = self.add()?;
                return Ok(Expr::Cmp(op, Box::new(a), Box::new(b)));
            }
        }
        Ok(a)
    }
    fn add(&mut self) -> Result<Expr, String> {
        let mut a = self.mul()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    a = Expr::Bin('+', Box::new(a), Box::new(self.mul()?))
                }
                Some(b'-') => {
                    self.i += 1;
                    a = Expr::Bin('-', Box::new(a), Box::new(self.mul()?))
                }
                _ => return Ok(a),
            }
        }
    }
    fn mul(&mut self) -> Result<Expr, String> {
        let mut a = self.unary()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    a = Expr::Bin('*', Box::new(a), Box::new(self.unary()?))
                }
                Some(b'/') => {
                    self.i += 1;
                    a = Expr::Bin('/', Box::new(a), Box::new(self.unary()?))
                }
                _ => return Ok(a),
            }
        }
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(b'-') {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.pow()
    }
    fn pow(&mut self) -> Result<Expr, String> {
        let a = self.atom()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let b = self.unary()?;
            return Ok(Expr::Bin('^', Box::new(a), Box::new(b)));
        }
        Ok(a)
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let e = self.cond()?;
                if !self.eat(")") {
                    return Err("expected ')'".into());
                }
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let st = self.i;
                while self.i < self.s.len() {
                    let c = self.s[self.i];
                    let prev = if self.i > st { self.s[self.i - 1] } else { b' ' };
                    if c.is_ascii_digit() || c == b'.' || c == b'e' || ((c == b'-' || c == b'+') && prev == b'e') {
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).unwrap();
                t.parse().map(Expr::Num).map_err(|_| format!("bad number {t}"))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let st = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_' || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[st..self.i]).unwrap().to_string();
                if self.peek() == Some(b'(') {
                    self.i += 1;
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.cond()?);
                            if self.eat(")") {
                                break;
                            }
                            if !self.eat(",") {
                                return Err("expected ','".into());
                            }
                        }
                    }
                    return Ok(Expr::Call(name, args));
                }
                Ok(Expr::Var(name))
            }
            c => Err(format!("unexpected {:?} at {}", c.map(|c| c as char), self.i)),
        }
    }
}

pub fn parse(s: &str) -> Result<Expr, String> {
    let mut p = P { s: s.as_bytes(), i: 0 };
    let e = p.cond()?;
    p.ws();
    if p.i != s.len() {
        return Err(format!("trailing input in '{s}' at {}", p.i));
    }
    Ok(e)
}

fn builtin(name: &str, a: &[f64]) -> Option<f64> {
    Some(match (name, a.len()) {
        ("exp", 1) => a[0].exp(),
        ("ln", 1) => a[0].ln(),
        ("log10", 1) => a[0].log10(),
        ("sqrt", 1) => a[0].sqrt(),
        ("abs", 1) => a[0].abs(),
        ("asinh", 1) => a[0].asinh(),
        ("sin", 1) => a[0].sin(),
        ("cos", 1) => a[0].cos(),
        ("rad", 1) => a[0].to_radians(),
        ("floor", 1) => a[0].floor(),
        ("and", 2) => ((a[0] != 0.0) && (a[1] != 0.0)) as u8 as f64,
        ("or", 2) => ((a[0] != 0.0) || (a[1] != 0.0)) as u8 as f64,
        ("min", n) if n >= 1 => a.iter().cloned().fold(f64::INFINITY, f64::min),
        ("max", n) if n >= 1 => a.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        _ => return None,
    })
}

pub fn eval(e: &Expr, env: &mut dyn Env) -> Result<Option<f64>, String> {
    Ok(match e {
        Expr::Num(v) => Some(*v),
        Expr::Var(n) => env.var(n)?,
        Expr::Neg(a) => eval(a, env)?.map(|v| -v),
        Expr::Bin(op, a, b) => {
            let (Some(x), Some(y)) = (eval(a, env)?, eval(b, env)?) else { return Ok(None) };
            Some(match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                '/' => x / y,
                _ => x.powf(y),
            })
        }
        Expr::Cmp(op, a, b) => {
            let (Some(x), Some(y)) = (eval(a, env)?, eval(b, env)?) else { return Ok(None) };
            let r = match *op {
                "<" => x < y,
                "<=" => x <= y,
                ">" => x > y,
                ">=" => x >= y,
                "==" => x == y,
                _ => x != y,
            };
            Some(if r { 1.0 } else { 0.0 })
        }
        Expr::Cond(c, a, b) => match eval(c, env)? {
            None => None,
            Some(v) if v != 0.0 => eval(a, env)?,
            Some(_) => eval(b, env)?,
        },
        Expr::Call(name, args) => {
            if name == "null" {
                return Ok(None);
            }
            if name == "isnull" {
                return Ok(Some(if eval(&args[0], env)?.is_none() { 1.0 } else { 0.0 }));
            }
            if name == "coalesce" {
                for a in args {
                    if let Some(v) = eval(a, env)? {
                        return Ok(Some(v));
                    }
                }
                return Ok(None);
            }
            let vals: Vec<Option<f64>> = args.iter().map(|a| eval(a, env)).collect::<Result<_, _>>()?;
            if vals.iter().all(|v| v.is_some()) {
                let raw: Vec<f64> = vals.iter().map(|v| v.unwrap()).collect();
                if let Some(v) = builtin(name, &raw) {
                    return Ok(Some(v));
                }
            } else if builtin(name, &vec![0.0; vals.len()]).is_some() {
                return Ok(None);
            }
            env.call(name, &vals)?
        }
    })
}
