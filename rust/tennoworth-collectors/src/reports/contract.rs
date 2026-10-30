//! What a report may contain. Everything a client sends is checked here before
//! the store sees it; a batch with one bad row is refused whole, so a client bug
//! surfaces as a 400 instead of as quietly missing data.

use chrono::{Datelike, Days, NaiveDate};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_SALES_PER_BATCH: usize = 50;
pub const MAX_QTY: u32 = 999;
pub const MAX_PLAT: u32 = 100_000;
/// A sale observed offline stays queued on the client for up to this many days.
pub const MAX_SALE_AGE_DAYS: u64 = 7;
const MAX_SLUG_LEN: usize = 96;
const MAX_SHAPE_ENTRIES: usize = 32;
const MAX_NAME_LEN: usize = 64;
const MAX_TAG_LEN: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub struct Invalid(pub &'static str);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SalesBatch {
    pub v: u32,
    pub contributor: String,
    pub erase: String,
    pub sales: Vec<Sale>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sale {
    /// Idempotency key: a resent batch must not count a sale twice.
    #[serde(rename = "ref")]
    pub reference: String,
    pub day: String,
    pub side: Side,
    pub slug: String,
    /// The item has a rank or subtype, which EE.log does not record.
    pub tiered: bool,
    pub qty: u32,
    pub plat: u32,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Sale,
    Purchase,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Sale => "sale",
            Side::Purchase => "purchase",
        }
    }
}

#[derive(Debug)]
pub struct ValidSale {
    pub reference: String,
    pub day: NaiveDate,
    pub side: Side,
    pub slug: String,
    pub tiered: bool,
    pub qty: u32,
    pub plat: u32,
}

#[derive(Debug)]
pub struct ValidBatch {
    pub week: String,
    pub contributor: String,
    pub erase: String,
    pub sales: Vec<ValidSale>,
}

pub fn iso_week(day: NaiveDate) -> String {
    let w = day.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SLUG_LEN
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn check_identity(v: u32, contributor: &str, erase: &str) -> Result<(), Invalid> {
    if v != CONTRACT_VERSION {
        return Err(Invalid("unsupported contract version"));
    }
    if !is_hex64(contributor) || !is_hex64(erase) {
        return Err(Invalid("contributor and erase must be 64 lowercase hex"));
    }
    Ok(())
}

pub fn validate_sales(batch: SalesBatch, today: NaiveDate) -> Result<ValidBatch, Invalid> {
    check_identity(batch.v, &batch.contributor, &batch.erase)?;
    if batch.sales.is_empty() || batch.sales.len() > MAX_SALES_PER_BATCH {
        return Err(Invalid("a batch holds 1 to 50 sales"));
    }
    let oldest = today
        .checked_sub_days(Days::new(MAX_SALE_AGE_DAYS))
        .ok_or(Invalid("date out of range"))?;
    // A client clock a little ahead of the server's must not lose a sale.
    let newest = today
        .checked_add_days(Days::new(1))
        .ok_or(Invalid("date out of range"))?;
    let mut week: Option<String> = None;
    let mut sales = Vec::with_capacity(batch.sales.len());
    for sale in batch.sales {
        let day = NaiveDate::parse_from_str(&sale.day, "%Y-%m-%d")
            .map_err(|_| Invalid("day must be YYYY-MM-DD"))?;
        if day < oldest || day > newest {
            return Err(Invalid("day outside the accepted window"));
        }
        // The contributor id is derived per ISO week, so one batch is one week.
        let this_week = iso_week(day);
        if week.as_ref().is_some_and(|w| *w != this_week) {
            return Err(Invalid("a batch covers one ISO week"));
        }
        week = Some(this_week);
        if !is_hex64(&sale.reference) {
            return Err(Invalid("ref must be 64 lowercase hex"));
        }
        if !is_slug(&sale.slug) {
            return Err(Invalid("slug must be lowercase letters, digits and _"));
        }
        if sale.qty == 0 || sale.qty > MAX_QTY {
            return Err(Invalid("qty out of range"));
        }
        if sale.plat == 0 || sale.plat > MAX_PLAT {
            return Err(Invalid("plat out of range"));
        }
        sales.push(ValidSale {
            reference: sale.reference,
            day,
            side: sale.side,
            slug: sale.slug,
            tiered: sale.tiered,
            qty: sale.qty,
            plat: sale.plat,
        });
    }
    Ok(ValidBatch {
        week: week.ok_or(Invalid("a batch holds 1 to 50 sales"))?,
        contributor: batch.contributor,
        erase: batch.erase,
        sales,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeReport {
    pub v: u32,
    pub contributor: String,
    pub erase: String,
    pub keys: Vec<ShapeKey>,
    pub tags: Vec<String>,
}

#[derive(Deserialize, serde::Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ShapeKey {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ValueKind,
    /// Key names of the objects inside an array or object value. Never values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

#[derive(Deserialize, serde::Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    String,
    Number,
    Boolean,
    Array,
    Object,
    Null,
}

#[derive(Debug)]
pub struct ValidShape {
    pub contributor: String,
    pub erase: String,
    /// Canonical JSON: sorted and deduplicated, so equal shapes compare equal.
    pub shape: String,
}

fn is_key_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    s.len() <= MAX_NAME_LEN
        && bytes.next().is_some_and(|c| c.is_ascii_alphabetic())
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// A game path such as `/Lotus/Upgrades/Mods/Randomized/...`. The pattern is
/// strict because a tag is the only free-form string a shape report carries.
fn is_tag(s: &str) -> bool {
    s.len() <= MAX_TAG_LEN
        && s.strip_prefix("/Lotus/").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'/' || c == b'_')
        })
}

pub fn validate_shape(report: ShapeReport) -> Result<ValidShape, Invalid> {
    check_identity(report.v, &report.contributor, &report.erase)?;
    if report.keys.len() > MAX_SHAPE_ENTRIES || report.tags.len() > MAX_SHAPE_ENTRIES {
        return Err(Invalid("too many keys or tags"));
    }
    if report.keys.is_empty() && report.tags.is_empty() {
        return Err(Invalid("a shape report names at least one key or tag"));
    }
    let mut keys = report.keys;
    for key in &mut keys {
        if !is_key_name(&key.name)
            || key.fields.len() > MAX_SHAPE_ENTRIES
            || !key.fields.iter().all(|f| is_key_name(f))
        {
            return Err(Invalid("key names are short identifiers"));
        }
        key.fields.sort();
        key.fields.dedup();
    }
    keys.sort();
    keys.dedup();
    let mut tags = report.tags;
    if !tags.iter().all(|t| is_tag(t)) {
        return Err(Invalid("tags are /Lotus/ game paths"));
    }
    tags.sort();
    tags.dedup();
    // A struct, not a json! map: map key order follows serde_json's
    // `preserve_order` feature, which any crate in a workspace build can enable.
    #[derive(serde::Serialize)]
    struct Canonical<'a> {
        keys: &'a [ShapeKey],
        tags: &'a [String],
    }
    let shape = serde_json::to_string(&Canonical {
        keys: &keys,
        tags: &tags,
    })
    .map_err(|_| Invalid("shape does not serialize"))?;
    Ok(ValidShape {
        contributor: report.contributor,
        erase: report.erase,
        shape,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EraseRequest {
    pub v: u32,
    pub week: String,
    pub secret: String,
}

pub struct ValidErase {
    pub week: String,
    /// What reports carried as `erase`: the SHA-256 of the secret's hex text.
    pub erase: String,
}

fn is_week(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 8
        && b.get(4) == Some(&b'-')
        && b.get(5) == Some(&b'W')
        && s.get(..4)
            .is_some_and(|y| y.bytes().all(|c| c.is_ascii_digit()))
        && s.get(6..)
            .and_then(|w| w.parse::<u32>().ok())
            .is_some_and(|w| (1..=53).contains(&w))
}

pub fn erase_hash(secret: &str) -> String {
    Sha256::digest(secret.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn validate_erase(request: EraseRequest) -> Result<ValidErase, Invalid> {
    if request.v != CONTRACT_VERSION {
        return Err(Invalid("unsupported contract version"));
    }
    if !is_week(&request.week) || !is_hex64(&request.secret) {
        return Err(Invalid("week is YYYY-Www and secret 64 lowercase hex"));
    }
    Ok(ValidErase {
        erase: erase_hash(&request.secret),
        week: request.week,
    })
}
