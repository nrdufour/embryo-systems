//! The names the system provides, as the polycopié lists them: each with
//! the page that does. The print calls its lists partial ("there are many
//! other multifunctors", page 77; "many other list functors", page 80), so
//! a name missing here is only warned about.

/// Built-in functors, upper case, with their page.
pub const FUNCTORS: &[(&str, u16)] = &[
    // Direct functors, page 76.
    ("ATT", 76),
    ("VATT", 76),
    ("OBJ", 76),
    ("PSC", 76),
    ("NSC", 76),
    ("EFF", 76),
    ("OATT", 76),
    ("OPSC", 76),
    ("ONSC", 76),
    // Reverse functors, page 76.
    ("ATTOF", 76),
    ("VATTOF", 76),
    ("OBJOF", 76),
    ("INI", 76),
    ("FIN", 76),
    ("EFFOF", 76),
    ("OATTOF", 76),
    ("OINI", 76),
    ("OFIN", 76),
    // Functors that keep the type, page 76.
    ("CONT", 76),
    ("CONTOF", 76),
    ("INV", 76),
    ("OINV", 76),
    // Multifunctors, page 77, and SIGMA, page 78.
    ("OPSCR", 77),
    ("OPSCRC", 77),
    ("ONSCRC", 77),
    ("OATTN", 77),
    ("SIGMA", 78),
    // List functors, page 80.
    ("FIRST", 80),
    ("LAST", 80),
    ("PRED", 80),
    ("SUCC", 80),
    ("CURRENT", 80),
    ("FIRSTOUT", 80),
    ("LASTOUT", 80),
    ("CURRENTOUT", 80),
    ("POSLP", 80),
    ("SUBLIST", 80),
    ("OCCUR", 80),
    ("CARD", 80),
    ("KIND", 80),
    ("TYPE", 80),
    ("ELMT", 80),
    // Expert system functors, page 80.
    ("INFACT", 80),
    ("OUTFACT", 80),
    ("INRULE", 80),
    ("OUTRULE", 80),
];

/// Properties of an a.d.t., set with LET: pages 156 to 158.
pub const PROPERTIES: &[(&str, u16)] = &[("COND", 156), ("ACTION", 157)];

pub fn is_functor(key: &str) -> bool {
    FUNCTORS.iter().any(|(name, _)| *name == key)
}

pub fn is_property(key: &str) -> bool {
    PROPERTIES.iter().any(|(name, _)| *name == key)
}
