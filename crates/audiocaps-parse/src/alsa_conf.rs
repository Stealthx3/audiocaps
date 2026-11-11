// SPDX-License-Identifier: LGPL-2.1-or-later
//! ALSA configuration: a parser for the alsa-lib configuration syntax (as written by
//! `snd_config_save`, or in `asound.conf`), and a resolver that follows a PCM name such as
//! `default` through its plugin chain without opening anything.
//!
//! Resolution is best effort: it follows aliases, `slave.pcm`, `asym` playback/capture slaves and
//! the common `@func` forms (`refer`, `concat`, `getenv` defaults, `card_driver`, `card_id`), and
//! substitutes `@args`. Anything else stops the chain with a reason, never a guess.

use std::collections::BTreeMap;

/// A configuration node: a leaf value or a compound of named children (arrays are compounds with
/// keys `0`, `1`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A string, integer or real, kept as text.
    Leaf(String),
    /// Named children in file order.
    Compound(Vec<(String, Node)>),
}

impl Node {
    fn empty() -> Self {
        Node::Compound(Vec::new())
    }

    /// The child named `key`, for a compound.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Compound(c) => c.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            Node::Leaf(_) => None,
        }
    }

    /// Follows a dotted path (`a.b.c`) without alias resolution.
    pub fn path(&self, path: &str) -> Option<&Node> {
        path.split('.')
            .filter(|p| !p.is_empty())
            .try_fold(self, |n, k| n.get(k))
    }

    /// The leaf text, for a leaf.
    pub fn leaf(&self) -> Option<&str> {
        match self {
            Node::Leaf(s) => Some(s),
            Node::Compound(_) => None,
        }
    }

    /// Children of a compound (empty for a leaf).
    pub fn children(&self) -> &[(String, Node)] {
        match self {
            Node::Compound(c) => c,
            Node::Leaf(_) => &[],
        }
    }

    fn child_mut(&mut self, key: &str, mode: Mode) -> &mut Node {
        if let Node::Leaf(_) = self {
            *self = Node::empty();
        }
        let Node::Compound(c) = self else {
            unreachable!()
        };
        let pos = c.iter().position(|(k, _)| k == key);
        match (pos, mode) {
            (Some(i), Mode::Override) => {
                c[i].1 = Node::empty();
                &mut c[i].1
            }
            (Some(i), _) => &mut c[i].1,
            (None, _) => {
                c.push((key.to_string(), Node::empty()));
                let last = c.len() - 1;
                &mut c[last].1
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Merge,
    Override,
    Default,
}

/// Parse errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfError {
    /// Unbalanced braces or brackets, or a missing value.
    Syntax {
        /// Byte offset.
        at: usize,
    },
    /// Nesting deeper than the parser allows.
    TooDeep,
}

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    /// A key written with quoted components, e.g. `USB-Audio."Audiophile USB (tm)".pcm.default`.
    Key(Vec<String>),
    Open(char),
    Close(char),
    Eq,
    Include(String),
}

fn tokenize(text: &str) -> Vec<(usize, Tok)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' | b',' | b';' => i += 1,
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' | b'[' => {
                out.push((i, Tok::Open(c as char)));
                i += 1;
            }
            b'}' | b']' => {
                out.push((i, Tok::Close(c as char)));
                i += 1;
            }
            b'=' => {
                out.push((i, Tok::Eq));
                i += 1;
            }
            b'<' => {
                // Include directive: `<file>` or `<confdir:pcm/front.conf>`.
                let start = i;
                i += 1;
                let s = i;
                while i < b.len() && b[i] != b'>' {
                    i += 1;
                }
                let path = String::from_utf8_lossy(&b[s..i.min(b.len())]).into_owned();
                i += 1;
                out.push((start, Tok::Include(path)));
            }
            _ => {
                let start = i;
                let (segs, next) = read_word(b, text, i);
                i = next;
                if segs.len() == 1 && !segs[0].0 {
                    out.push((
                        start,
                        Tok::Word(segs.into_iter().next().unwrap_or_default().1),
                    ));
                } else {
                    let mut parts = Vec::new();
                    for (quoted, seg) in segs {
                        if quoted {
                            parts.push(seg);
                        } else {
                            parts
                                .extend(seg.split('.').filter(|p| !p.is_empty()).map(String::from));
                        }
                    }
                    out.push((start, Tok::Key(parts)));
                }
            }
        }
    }
    out
}

fn is_sep(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t' | b'\r' | b'\n' | b',' | b';' | b'{' | b'}' | b'[' | b']' | b'=' | b'#'
    )
}

/// Reads one word: unquoted runs and quoted strings joined without separators (a key such as
/// `a."b c".d`). Returns the segments (quoted or not) and the next position.
fn read_word(b: &[u8], text: &str, mut i: usize) -> (Vec<(bool, String)>, usize) {
    let mut segs: Vec<(bool, String)> = Vec::new();
    while i < b.len() && !is_sep(b[i]) {
        if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            i += 1;
            let mut s = Vec::new();
            while i < b.len() && b[i] != q {
                if b[i] == b'\\' && i + 1 < b.len() {
                    i += 1;
                    s.push(match b[i] {
                        b'n' => b'\n',
                        b't' => b'\t',
                        other => other,
                    });
                } else {
                    s.push(b[i]);
                }
                i += 1;
            }
            i += 1;
            segs.push((true, String::from_utf8_lossy(&s).into_owned()));
        } else {
            let start = i;
            while i < b.len() && !is_sep(b[i]) && b[i] != b'"' && b[i] != b'\'' {
                i += 1;
            }
            segs.push((false, text[start..i].to_string()));
        }
        // A quoted part continues the word only when joined by a dot.
        let joined = match (segs.last(), b.get(i)) {
            (Some((true, _)), Some(b'.')) => true,
            (Some((false, s)), Some(b'"' | b'\'')) => s.ends_with('.'),
            _ => false,
        };
        if !joined {
            break;
        }
    }
    (segs, i)
}

/// Reads an included file by absolute path. `confdir:` paths are mapped to `/usr/share/alsa/`.
pub type Loader<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Directory alsa-lib calls `confdir` and `datadir`.
pub const DATADIR: &str = "/usr/share/alsa";

const MAX_INCLUDES: usize = 64;

struct Parser<'a> {
    loader: Option<Loader<'a>>,
    includes: usize,
}

/// Parses configuration text into a root compound. Include directives are skipped.
pub fn parse(text: &str) -> Result<Node, ConfError> {
    let mut root = Node::empty();
    Parser {
        loader: None,
        includes: 0,
    }
    .parse_into(text, &mut root, 0)?;
    Ok(root)
}

/// Parses configuration text, following include directives through `loader`.
pub fn parse_with(text: &str, loader: Loader<'_>) -> Result<Node, ConfError> {
    let mut root = Node::empty();
    Parser {
        loader: Some(loader),
        includes: 0,
    }
    .parse_into(text, &mut root, 0)?;
    Ok(root)
}

/// Merges `src` into `dst` (children of compounds merge; leaves replace).
pub fn merge(dst: &mut Node, src: Node) {
    match (dst, src) {
        (d @ Node::Leaf(_), s) => *d = s,
        (d, s @ Node::Leaf(_)) => *d = s,
        (Node::Compound(dc), Node::Compound(sc)) => {
            for (k, v) in sc {
                match dc.iter_mut().find(|(dk, _)| *dk == k) {
                    Some((_, dv)) => merge(dv, v),
                    None => dc.push((k, v)),
                }
            }
        }
    }
}

impl Parser<'_> {
    fn parse_into(&mut self, text: &str, into: &mut Node, depth: usize) -> Result<(), ConfError> {
        let toks = tokenize(text);
        let mut pos = 0;
        self.body(&toks, &mut pos, into, None, depth)
    }

    fn include(&mut self, path: &str, into: &mut Node, depth: usize) -> Result<(), ConfError> {
        let Some(loader) = self.loader else {
            return Ok(());
        };
        if self.includes >= MAX_INCLUDES || depth > 8 {
            return Ok(());
        }
        self.includes += 1;
        let path = match path.strip_prefix("confdir:") {
            Some(rest) => format!("{DATADIR}/{rest}"),
            None => path.to_string(),
        };
        if let Some(text) = loader(&path) {
            // A broken include is skipped, as alsa-lib does with `errors false`.
            let _ = self.parse_into(&text, into, depth + 1);
        }
        Ok(())
    }

    fn body(
        &mut self,
        toks: &[(usize, Tok)],
        pos: &mut usize,
        into: &mut Node,
        close: Option<char>,
        depth: usize,
    ) -> Result<(), ConfError> {
        if depth > MAX_DEPTH {
            return Err(ConfError::TooDeep);
        }
        let mut index = 0usize;
        while *pos < toks.len() {
            let (at, tok) = &toks[*pos];
            match tok {
                Tok::Close(c) => {
                    return if Some(*c) == close {
                        *pos += 1;
                        Ok(())
                    } else {
                        Err(ConfError::Syntax { at: *at })
                    };
                }
                Tok::Include(path) => {
                    *pos += 1;
                    let path = path.clone();
                    self.include(&path, into, depth)?;
                }
                _ if close == Some(']') => {
                    // Array element: value without a key.
                    let key = index.to_string();
                    index += 1;
                    let slot = into.child_mut(&key, Mode::Merge);
                    self.value(toks, pos, slot, depth)?;
                }
                Tok::Word(_) | Tok::Key(_) => {
                    *pos += 1;
                    if matches!(toks.get(*pos), Some((_, Tok::Eq))) {
                        *pos += 1;
                    }
                    let comps: Vec<&str> = match tok {
                        Tok::Key(parts) => parts.iter().map(String::as_str).collect(),
                        Tok::Word(w) => w.split('.').filter(|p| !p.is_empty()).collect(),
                        _ => unreachable!(),
                    };
                    // Each dotted component may carry a mode prefix: `pcm.!default`, `!pcm.x`.
                    let parts: Vec<(Mode, &str)> = comps
                        .into_iter()
                        .map(|p| match p.as_bytes().first() {
                            Some(b'!') => (Mode::Override, &p[1..]),
                            Some(b'?') => (Mode::Default, &p[1..]),
                            Some(b'+' | b'-') => (Mode::Merge, &p[1..]),
                            _ => (Mode::Merge, p),
                        })
                        .collect();
                    if parts.is_empty() || parts.iter().any(|(_, k)| k.is_empty()) {
                        return Err(ConfError::Syntax { at: *at });
                    }
                    let plain: Vec<&str> = parts.iter().map(|(_, k)| *k).collect();
                    if parts.iter().any(|(m, _)| *m == Mode::Default)
                        && into.path(&plain.join(".")).is_some()
                    {
                        let mut scratch = Node::empty();
                        self.value(toks, pos, &mut scratch, depth)?;
                        continue;
                    }
                    let mut slot: &mut Node = into;
                    for (m, k) in &parts {
                        slot = slot.child_mut(k, *m);
                    }
                    self.value(toks, pos, slot, depth)?;
                }
                Tok::Open(_) | Tok::Eq => return Err(ConfError::Syntax { at: *at }),
            }
        }
        if close.is_some() {
            Err(ConfError::Syntax { at: usize::MAX })
        } else {
            Ok(())
        }
    }

    fn value(
        &mut self,
        toks: &[(usize, Tok)],
        pos: &mut usize,
        slot: &mut Node,
        depth: usize,
    ) -> Result<(), ConfError> {
        match toks.get(*pos) {
            Some((_, Tok::Word(v))) => {
                *slot = Node::Leaf(v.clone());
                *pos += 1;
                Ok(())
            }
            Some((_, Tok::Key(parts))) => {
                *slot = Node::Leaf(parts.join("."));
                *pos += 1;
                Ok(())
            }
            Some((_, Tok::Open(c))) => {
                let close = if *c == '{' { '}' } else { ']' };
                *pos += 1;
                if let Node::Leaf(_) = slot {
                    *slot = Node::empty();
                }
                if close == ']' {
                    // Arrays replace rather than merge.
                    *slot = Node::empty();
                }
                self.body(toks, pos, slot, Some(close), depth + 1)
            }
            Some((at, _)) => Err(ConfError::Syntax { at: *at }),
            None => Err(ConfError::Syntax { at: usize::MAX }),
        }
    }
}

/// Builds the `cards` subtree the way alsa-lib's `cards.@hooks` does when a PCM is opened:
/// loads `cards/aliases.conf` (which includes `pcm/default.conf`, `pcm/dmix.conf`, …), then each
/// card's `cards/<driver>.conf`, and adds a `cards.<index>` alias per card. Files come from
/// `loader`; missing files are skipped.
pub fn load_cards(root: &mut Node, cards: &[CardRef], loader: Loader<'_>) {
    let mut ct = Node::empty();
    if let Some(t) = loader(&format!("{DATADIR}/cards/aliases.conf"))
        && let Ok(n) = parse_with(&t, loader)
    {
        merge(&mut ct, n);
    }
    for c in cards {
        let mut driver = c.driver.clone();
        if let Some(Node::Leaf(a)) = ct.get(&driver)
            && let Some(x) = a.strip_prefix("cards.")
        {
            driver = x.to_string();
        }
        if driver.is_empty()
            || !driver
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            continue;
        }
        if let Some(t) = loader(&format!("{DATADIR}/cards/{driver}.conf"))
            && let Ok(n) = parse_with(&t, loader)
        {
            merge(&mut ct, n);
        }
        *ct.child_mut(&c.index.to_string(), Mode::Override) = Node::Leaf(format!("cards.{driver}"));
    }
    *root.child_mut("cards", Mode::Override) = ct;
}

/// A sound card, as the resolver needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardRef {
    /// Index.
    pub index: u32,
    /// Id string, e.g. `PCH`.
    pub id: String,
    /// Driver, e.g. `HDA-Intel`.
    pub driver: String,
    /// Short name, e.g. `HDA Intel PCH` (`card_name`).
    pub name: String,
}

/// Stream direction for resolution (`asym` picks its slave by it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Playback.
    Playback,
    /// Capture.
    Capture,
}

/// One stage of a resolved chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    /// The PCM name at this stage, e.g. `default`, `dmix:CARD=0`.
    pub name: String,
    /// Plugin type, e.g. `plug`, `dmix`, `hw`, `pulse`, `pipewire`.
    pub kind: String,
    /// Card the stage names (id or index as configured, resolved to the id when possible).
    pub card: Option<String>,
    /// Device the stage names.
    pub device: Option<u32>,
    /// Fixed rate (dmix/dsnoop `slave.rate`).
    pub rate: Option<u32>,
}

/// A resolved chain, application side first. `stopped` says why resolution ended early.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chain {
    /// Stages.
    pub stages: Vec<Stage>,
    /// Why the chain is incomplete, if it is.
    pub stopped: Option<String>,
}

const MAX_HOPS: usize = 16;

/// Resolves PCM `name` (e.g. `default`, `plughw:1,0`) against configuration `root`.
pub fn resolve_pcm(root: &Node, name: &str, stream: Stream, cards: &[CardRef]) -> Chain {
    let mut chain = Chain::default();
    let mut r = Resolver {
        root,
        cards,
        stream,
        env: BTreeMap::new(),
        hops: 0,
    };
    let mut next = Some(PcmRef::Name(name.to_string()));
    while let Some(cur) = next.take() {
        if chain.stages.len() >= MAX_HOPS {
            chain.stopped = Some("too many stages".into());
            break;
        }
        match r.step(cur) {
            Ok((stage, slave)) => {
                let terminal = slave.is_none();
                chain.stages.push(stage);
                next = slave;
                if terminal {
                    break;
                }
            }
            Err(e) => {
                chain.stopped = Some(e);
                break;
            }
        }
    }
    chain
}

/// A PCM to resolve: by name, or an inline definition.
#[derive(Debug, Clone)]
enum PcmRef {
    /// A PCM name, looked up under `pcm.` (e.g. `dmix:CARD=0`).
    Name(String),
    /// A configuration path from the root, as `@func refer` names it (e.g.
    /// `cards.0.pcm.default:CARD=0`).
    Path(String),
    /// An inline definition with its bound arguments.
    Inline(Node, BTreeMap<String, String>),
}

struct Resolver<'a> {
    root: &'a Node,
    cards: &'a [CardRef],
    stream: Stream,
    env: BTreeMap<String, String>,
    /// Definitions followed so far (bounds `@func refer` chains).
    hops: usize,
}

impl Resolver<'_> {
    /// Searches a dotted path from the root, following string aliases at each component.
    fn search(&self, path: &str, hops: usize) -> Option<&Node> {
        if hops > MAX_HOPS {
            return None;
        }
        let mut node = self.root;
        let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
        for (i, p) in parts.iter().enumerate() {
            node = node.get(p)?;
            if let Node::Leaf(alias) = node
                && i + 1 < parts.len()
            {
                let rest = parts[i + 1..].join(".");
                return self.search(&format!("{alias}.{rest}"), hops + 1);
            }
        }
        Some(node)
    }

    fn step(&mut self, cur: PcmRef) -> Result<(Stage, Option<PcmRef>), String> {
        let (display, def, args) = match cur {
            PcmRef::Name(name) => {
                let (base, argstr) = match name.split_once(':') {
                    Some((b, a)) => (b.to_string(), Some(a.to_string())),
                    None => (name.clone(), None),
                };
                let mut hops = 0;
                let mut base_name = base.clone();
                let mut argstr = argstr;
                let def = loop {
                    let n = self
                        .search(&format!("pcm.{base_name}"), 0)
                        .ok_or_else(|| format!("pcm.{base_name} is not defined"))?;
                    match n {
                        Node::Leaf(_) if hops >= MAX_HOPS => return Err("alias loop".into()),
                        Node::Leaf(alias) => {
                            hops += 1;
                            // `pcm.default cards.pcm.default`: an alias to a config path.
                            if let Some(p @ Node::Compound(_)) = self.search(alias, 0) {
                                break p.clone();
                            }
                            // `pcm.foo "hw:1,0"`: an alias to another PCM name.
                            match alias.split_once(':') {
                                Some((b, a)) => {
                                    base_name = b.to_string();
                                    argstr = Some(a.to_string());
                                }
                                None => base_name = alias.clone(),
                            }
                        }
                        Node::Compound(_) => break n.clone(),
                    }
                };
                let args = self.bind_args(&def, argstr.as_deref());
                (name, def, args)
            }
            PcmRef::Path(path) => {
                let (base, argstr) = match path.split_once(':') {
                    Some((b, a)) => (b, Some(a)),
                    None => (path.as_str(), None),
                };
                let def = match self.search(base, 0) {
                    Some(n @ Node::Compound(_)) => n.clone(),
                    Some(Node::Leaf(alias)) => {
                        let alias = alias.clone();
                        return self.step(PcmRef::Name(alias));
                    }
                    None => return Err(format!("{base} is not defined")),
                };
                let args = self.bind_args(&def, argstr);
                (path.clone(), def, args)
            }
            PcmRef::Inline(def, args) => (String::new(), def, args),
        };
        self.hops += 1;
        if self.hops > 4 * MAX_HOPS {
            return Err("definition chain too long".into());
        }
        let saved = std::mem::replace(&mut self.env, args);
        // A definition can itself be `@func refer` (USB-Audio: a per-device variant, else a
        // default): follow it with the arguments bound.
        let res = match def.get("@func").and_then(Node::leaf) {
            Some("refer") => self.refer(&def).and_then(|next| self.step(next)),
            // Any other function evaluates to the name of the PCM to use (`dmix:CARD`).
            Some(_) => self
                .func(&def, 0)
                .and_then(|name| self.step(PcmRef::Name(name))),
            None => self.describe(&display, &def),
        };
        self.env = saved;
        res
    }

    fn describe(&mut self, display: &str, def: &Node) -> Result<(Stage, Option<PcmRef>), String> {
        let kind = self
            .value(def.get("type"))
            .ok_or_else(|| format!("{} has no type", display_or(display)))?;
        let card = self
            .value(def.get("card"))
            .or_else(|| self.value(def.path("slave.pcm.card")))
            .map(|c| self.card_name(&c));
        let device = self.value(def.get("device")).and_then(|d| d.parse().ok());
        let rate = self
            .value(def.path("slave.rate"))
            .and_then(|r| r.parse().ok());
        let mut stage = Stage {
            name: if display.is_empty() {
                format!("<inline {kind}>")
            } else {
                display.to_string()
            },
            kind: kind.clone(),
            card,
            device,
            rate,
        };
        let slave_node = match kind.as_str() {
            "asym" => {
                let key = match self.stream {
                    Stream::Playback => "playback.pcm",
                    Stream::Capture => "capture.pcm",
                };
                def.path(key)
            }
            "hw" | "pulse" | "pipewire" | "jack" | "null" | "file" | "bluealsa" | "oss" => None,
            _ => def.path("slave.pcm").or_else(|| def.get("slave")),
        };
        // Some plugins (dmix, dsnoop) name their hardware via slave.pcm compound with type hw.
        let Some(slave_node) = slave_node else {
            return Ok((stage, None));
        };
        let slave = match slave_node {
            Node::Leaf(_) => {
                let v = self
                    .value(Some(slave_node))
                    .ok_or_else(|| "unresolved slave".to_string())?;
                PcmRef::Name(v)
            }
            Node::Compound(_) => {
                if slave_node.get("@func").and_then(Node::leaf) == Some("refer") {
                    self.refer(slave_node)?
                } else if slave_node.get("@func").is_some() {
                    PcmRef::Name(self.func(slave_node, 0)?)
                } else if slave_node.get("type").is_some() {
                    PcmRef::Inline(slave_node.clone(), self.env.clone())
                } else if let Some(p) = slave_node.get("pcm") {
                    PcmRef::Name(self.value(Some(p)).ok_or("unresolved slave pcm")?)
                } else {
                    return Err("slave without pcm".into());
                }
            }
        };
        if stage.card.is_none()
            && let PcmRef::Inline(n, _) = &slave
        {
            stage.card = self.value(n.get("card")).map(|c| self.card_name(&c));
            stage.device = self.value(n.get("device")).and_then(|d| d.parse().ok());
        }
        Ok((stage, Some(slave)))
    }

    /// `@func refer` as a slave: the named definition if it exists, else the inline default.
    fn refer(&self, n: &Node) -> Result<PcmRef, String> {
        let name = match n.get("name") {
            Some(c @ Node::Compound(_)) => self.func(c, 1)?,
            Some(Node::Leaf(s)) => self.subst(s).ok_or("unbound refer name")?,
            None => return Err("@func refer without name".into()),
        };
        let base = name.split_once(':').map_or(name.as_str(), |(b, _)| b);
        if self.search(base, 0).is_some() {
            return Ok(PcmRef::Path(name));
        }
        match n.get("default") {
            Some(d @ Node::Compound(_)) => Ok(PcmRef::Inline(d.clone(), self.env.clone())),
            Some(Node::Leaf(s)) => Ok(PcmRef::Name(self.subst(s).ok_or("unbound default")?)),
            None => Err(format!("{base} is not defined")),
        }
    }

    /// The text value of a node, substituting `$ARG` and evaluating `@func`.
    fn value(&self, n: Option<&Node>) -> Option<String> {
        match n? {
            Node::Leaf(s) => self.subst(s),
            c @ Node::Compound(_) if c.get("@func").is_some() => self.func(c, 0).ok(),
            Node::Compound(_) => None,
        }
    }

    fn subst(&self, s: &str) -> Option<String> {
        match s.strip_prefix('$') {
            Some(var) => self.env.get(var).cloned(),
            None => Some(s.to_string()),
        }
    }

    fn func(&self, n: &Node, depth: usize) -> Result<String, String> {
        if depth > 8 {
            return Err("function nesting too deep".into());
        }
        let f = n.get("@func").and_then(Node::leaf).unwrap_or("");
        let arg = |k: &str| -> Result<String, String> {
            match n.get(k) {
                Some(c @ Node::Compound(_)) if c.get("@func").is_some() => self.func(c, depth + 1),
                Some(Node::Leaf(s)) => self.subst(s).ok_or_else(|| format!("${s} unbound")),
                _ => Err(format!("@func {f}: missing {k}")),
            }
        };
        match f {
            "concat" => {
                let strings = n.get("strings").ok_or("@func concat without strings")?;
                let mut out = String::new();
                for (_, v) in strings.children() {
                    match v {
                        Node::Leaf(s) => out.push_str(&self.subst(s).unwrap_or_default()),
                        c => out.push_str(&self.func(c, depth + 1)?),
                    }
                }
                Ok(out)
            }
            "refer" => {
                let name = arg("name")?;
                match self.search(&name, 0) {
                    Some(Node::Leaf(s)) => {
                        // A string that names another setting is an alias (alsa-lib follows it):
                        // `defaults.pcm.dmix.device defaults.pcm.device`.
                        let mut v = s.clone();
                        for _ in 0..8 {
                            match self.search(&v, 0) {
                                Some(Node::Leaf(next)) if v.contains('.') => v = next.clone(),
                                _ => break,
                            }
                        }
                        Ok(v)
                    }
                    Some(Node::Compound(_)) => Ok(name),
                    None => match n.get("default") {
                        Some(Node::Leaf(s)) => self.subst(s).ok_or_else(|| "unbound".into()),
                        _ => Err(format!("{name} is not defined")),
                    },
                }
            }
            "getenv" => match n.get("default") {
                // Environment variables are not read: the configured default is used.
                Some(Node::Leaf(s)) => self.subst(s).ok_or_else(|| "unbound".into()),
                Some(c) => self.func(c, depth + 1),
                None => Err("@func getenv without default".into()),
            },
            "card_driver" => {
                let c = arg("card")?;
                self.card(&c)
                    .map(|c| c.driver.clone())
                    .ok_or_else(|| format!("no card {c}"))
            }
            "card_name" => {
                let c = arg("card")?;
                self.card(&c)
                    .map(|c| c.name.clone())
                    .ok_or_else(|| format!("no card {c}"))
            }
            "card_inum" => {
                let c = arg("card")?;
                self.card(&c)
                    .map(|c| c.index.to_string())
                    .ok_or_else(|| format!("no card {c}"))
            }
            "card_id" => {
                let c = arg("card")?;
                self.card(&c)
                    .map(|c| c.id.clone())
                    .ok_or_else(|| format!("no card {c}"))
            }
            "datadir" => Ok("/usr/share/alsa".into()),
            other => Err(format!("@func {other} is not evaluated")),
        }
    }

    fn card(&self, c: &str) -> Option<&CardRef> {
        match c.parse::<u32>() {
            Ok(i) => self.cards.iter().find(|x| x.index == i),
            Err(_) => self.cards.iter().find(|x| x.id == c),
        }
    }

    fn card_name(&self, c: &str) -> String {
        self.card(c).map(|x| x.id.clone()).unwrap_or(c.to_string())
    }
}

fn display_or(s: &str) -> &str {
    if s.is_empty() { "inline pcm" } else { s }
}

impl Resolver<'_> {
    /// Binds `@args` of a definition: positional or `KEY=VALUE` arguments, then declared defaults
    /// (which may use `@func`, e.g. `refer defaults.pcm.card`).
    fn bind_args(&self, def: &Node, argstr: Option<&str>) -> BTreeMap<String, String> {
        let names: Vec<String> = def
            .get("@args")
            .map(|a| {
                a.children()
                    .iter()
                    .filter_map(|(k, v)| k.parse::<usize>().ok().and(v.leaf()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut env = BTreeMap::new();
        if let Some(s) = argstr {
            let s = s.trim_matches(|c| c == '{' || c == '}');
            for (i, part) in s.split(',').enumerate() {
                let part = part.trim().trim_matches('"');
                match part.split_once('=') {
                    Some((k, v)) => {
                        env.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
                    }
                    None if !part.is_empty() => {
                        if let Some(n) = names.get(i) {
                            env.insert(n.clone(), part.to_string());
                        }
                    }
                    None => {}
                }
            }
        }
        for n in &names {
            if env.contains_key(n) {
                continue;
            }
            if let Some(d) = def.path(&format!("@args.{n}.default")) {
                let r = Resolver {
                    root: self.root,
                    cards: self.cards,
                    stream: self.stream,
                    env: env.clone(),
                    hops: self.hops,
                };
                if let Some(v) = r.value(Some(d)) {
                    env.insert(n.clone(), v);
                }
            }
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
defaults.pcm.card 0
defaults.pcm.device 0
pcm.hw {
	@args [ CARD DEV SUBDEV ]
	@args.CARD { type string default { @func getenv vars [ ALSA_PCM_CARD ] default { @func refer name defaults.pcm.card } } }
	@args.DEV { type integer default 0 }
	@args.SUBDEV { type integer default -1 }
	type hw
	card $CARD
	device $DEV
	subdevice $SUBDEV
}
pcm.plughw {
	@args [ CARD DEV SUBDEV ]
	@args.CARD { type string default 0 }
	@args.DEV { type integer default 0 }
	type plug
	slave.pcm { type hw card $CARD device $DEV }
}
pcm.dmix {
	@args [ CARD DEV ]
	@args.CARD { type string default 0 }
	@args.DEV { type integer default 0 }
	type dmix
	slave { pcm { type hw card $CARD device $DEV } rate 48000 }
}
"#;

    fn cards() -> Vec<CardRef> {
        vec![
            CardRef {
                index: 0,
                id: "PCH".into(),
                driver: "HDA-Intel".into(),
                name: "PCH".into(),
            },
            CardRef {
                index: 1,
                id: "HDMI".into(),
                driver: "HDA-Intel".into(),
                name: "HDMI".into(),
            },
        ]
    }

    #[test]
    fn parses_syntax() {
        let t = parse("a.b 1\nc { d \"x y\"; e = [ 1 2 ] }\n# comment\n!f 3").unwrap();
        assert_eq!(t.path("a.b").and_then(Node::leaf), Some("1"));
        assert_eq!(t.path("c.d").and_then(Node::leaf), Some("x y"));
        assert_eq!(t.path("c.e.1").and_then(Node::leaf), Some("2"));
        assert_eq!(t.path("f").and_then(Node::leaf), Some("3"));
        assert!(parse("a { b 1").is_err());
        assert!(parse("a }").is_err());
    }

    #[test]
    fn override_and_default_modes() {
        let t = parse("a { x 1 y 2 }\n!a { z 3 }\n?b 1\n?b 2").unwrap();
        assert!(t.path("a.x").is_none());
        assert_eq!(t.path("a.z").and_then(Node::leaf), Some("3"));
        assert_eq!(t.path("b").and_then(Node::leaf), Some("1"));
    }

    #[test]
    fn pipewire_default() {
        let t = parse(&format!(
            "{BASE}\npcm.!default {{ type pipewire playback_node \"-1\" capture_node \"-1\" }}"
        ))
        .unwrap();
        let c = resolve_pcm(&t, "default", Stream::Playback, &cards());
        assert_eq!(c.stopped, None);
        assert_eq!(c.stages.len(), 1);
        assert_eq!(c.stages[0].kind, "pipewire");
    }

    #[test]
    fn plughw_and_dmix() {
        let t = parse(BASE).unwrap();
        let c = resolve_pcm(&t, "plughw:1,3", Stream::Playback, &cards());
        assert_eq!(c.stopped, None, "{c:?}");
        assert_eq!(
            c.stages.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(),
            ["plug", "hw"]
        );
        assert_eq!(c.stages[0].card.as_deref(), Some("HDMI"));
        assert_eq!(c.stages[1].device, Some(3));
        let d = resolve_pcm(&t, "dmix:CARD=PCH", Stream::Playback, &cards());
        assert_eq!(d.stages[0].kind, "dmix");
        assert_eq!(d.stages[0].rate, Some(48000));
        assert_eq!(d.stages[0].card.as_deref(), Some("PCH"));
        assert_eq!(d.stages[1].kind, "hw");
    }

    #[test]
    fn asym_picks_direction_and_refer() {
        let t = parse(&format!(
            "{BASE}\ncards.HDA-Intel.pcm.default {{ @args [ CARD ] @args.CARD {{ type string }} type asym playback.pcm {{ type plug slave.pcm \"dmix:CARD=$CARD\" }} capture.pcm \"hw:$CARD\" }}\n\
             pcm.!default {{ type empty slave.pcm {{ @func refer name {{ @func concat strings [ \"cards.\" {{ @func card_driver card 0 }} \".pcm.default:CARD=0\" ] }} }} }}"
        ))
        .unwrap();
        let p = resolve_pcm(&t, "default", Stream::Playback, &cards());
        assert_eq!(
            p.stages.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(),
            ["empty", "asym", "plug", "dmix", "hw"],
            "{p:?}"
        );
        let c = resolve_pcm(&t, "default", Stream::Capture, &cards());
        assert_eq!(
            c.stages.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(),
            ["empty", "asym", "hw"],
            "{c:?}"
        );
    }

    #[test]
    fn undefined_stops_with_reason() {
        let t = parse("pcm.default { type plug slave.pcm \"nothere\" }").unwrap();
        let c = resolve_pcm(&t, "default", Stream::Playback, &[]);
        assert_eq!(c.stages.len(), 1);
        assert!(c.stopped.unwrap().contains("nothere"));
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let t = "a { ".repeat(200);
        assert!(parse(&t).is_err());
    }

    #[test]
    fn mode_prefix_on_any_component() {
        let t = parse("pcm.default { type hw }\npcm.!default { type pulse }").unwrap();
        assert_eq!(
            t.path("pcm.default.type").and_then(Node::leaf),
            Some("pulse")
        );
    }

    #[test]
    fn includes_and_lazy_cards_tree() {
        let files: BTreeMap<&str, &str> = [
            (
                "/usr/share/alsa/cards/aliases.conf",
                "YMF724 cards.YMF744\n<confdir:pcm/default.conf>\n",
            ),
            (
                "/usr/share/alsa/pcm/default.conf",
                r#"pcm.!default {
    @args [ CARD ]
    @args.CARD { type string default { @func refer name defaults.pcm.card } }
    type empty
    slave.pcm {
        @func refer
        name { @func concat strings [ "cards." { @func card_inum card $CARD } ".pcm.default:CARD=" $CARD ] }
        default { type plug slave.pcm { type hw card $CARD } }
    }
}"#,
            ),
            (
                "/usr/share/alsa/cards/HDA-Intel.conf",
                r#"HDA-Intel.pcm.default {
    @args [ CARD ]
    @args.CARD { type string }
    type asym
    playback.pcm { type plug slave.pcm { type softvol slave.pcm { @func concat strings [ "dmix:" $CARD ] } } }
    capture.pcm { type plug slave.pcm { type hw card $CARD } }
}"#,
            ),
        ]
        .into_iter()
        .collect();
        let loader = |p: &str| files.get(p).map(|s| s.to_string());
        let dump = format!("{BASE}\npcm.default 'cards.pcm.default'\n");
        let mut t = parse(&dump).unwrap();
        load_cards(&mut t, &cards(), &loader);
        let p = resolve_pcm(&t, "default", Stream::Playback, &cards());
        assert_eq!(
            p.stages.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(),
            ["empty", "asym", "plug", "softvol", "dmix", "hw"],
            "{p:?}"
        );
        assert_eq!(p.stopped, None);
        assert_eq!(p.stages[4].rate, Some(48000));
        // A driver without its own file falls back to the inline default (plughw).
        let usb = vec![CardRef {
            index: 0,
            id: "Device".into(),
            driver: "USB-Audio".into(),
            name: "Device".into(),
        }];
        let mut t = parse(&dump).unwrap();
        load_cards(&mut t, &usb, &loader);
        let p = resolve_pcm(&t, "default", Stream::Playback, &usb);
        assert_eq!(
            p.stages.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(),
            ["empty", "plug", "hw"],
            "{p:?}"
        );
    }

    #[test]
    fn quoted_key_components() {
        let t = parse(
            "USB-Audio.\"Audiophile USB (tm)\".pcm.default { type asym }\nx \"a.b\"\n\"c.d\" 1\nUSB-Audio.pcm.use_dmix { \"NoiseBlaster 3000\" no }",
        )
        .unwrap();
        assert_eq!(
            t.get("USB-Audio")
                .and_then(|n| n.get("Audiophile USB (tm)"))
                .and_then(|n| n.path("pcm.default.type"))
                .and_then(Node::leaf),
            Some("asym")
        );
        assert!(t.get("pcm").is_none(), "no stray pcm.default at the root");
        assert_eq!(t.get("x").and_then(Node::leaf), Some("a.b"));
        assert_eq!(t.get("c.d").and_then(Node::leaf), Some("1"));
        assert_eq!(
            t.path("USB-Audio.pcm.use_dmix")
                .and_then(|n| n.get("NoiseBlaster 3000"))
                .and_then(Node::leaf),
            Some("no")
        );
    }
}
