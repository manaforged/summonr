use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

const SOURCE: &str = "src/catalog/models-dev.json";

pub(crate) fn compile() {
    println!("cargo:rerun-if-changed={SOURCE}");
    let text = std::fs::read_to_string(SOURCE)
        .unwrap_or_else(|error| panic!("cannot read {SOURCE}: {error}"));
    let root: Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{SOURCE} is invalid JSON: {error}"));
    let out =
        Path::new(&std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("models_dev.rs");
    std::fs::write(&out, emit(&root))
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}

#[derive(Default)]
struct Pool {
    strings: String,
    spans: BTreeMap<String, (u32, u16)>,
    prices: Vec<[u64; 4]>,
    price_ids: BTreeMap<[u64; 4], u16>,
}

impl Pool {
    fn str_span(&mut self, value: &str) -> (u32, u16) {
        if let Some(&span) = self.spans.get(value) {
            return span;
        }
        let off = u32::try_from(self.strings.len()).expect("bundled catalog string pool fits u32");
        let len = u16::try_from(value.len()).expect("bundled catalog string fits u16");
        assert!(len < u16::MAX, "u16::MAX is reserved for Option::None");
        self.strings.push_str(value);
        self.spans.insert(value.to_owned(), (off, len));
        (off, len)
    }

    fn opt_str(&mut self, value: Option<&str>) -> (u32, u16) {
        value.map_or((0, u16::MAX), |text| self.str_span(text))
    }

    fn opt_price(&mut self, value: Option<&Value>) -> u16 {
        let Some(price) = value.filter(|node| !node.is_null()) else {
            return u16::MAX;
        };
        let key = [
            req_f64(price, "input").to_bits(),
            req_f64(price, "output").to_bits(),
            req_f64(price, "cached").to_bits(),
            price
                .get("cache_write")
                .and_then(Value::as_f64)
                .map_or(u64::MAX, f64::to_bits),
        ];
        assert!(key[3] != u64::MAX || price.get("cache_write").is_none_or(Value::is_null));
        if let Some(&id) = self.price_ids.get(&key) {
            return id;
        }
        let id = u16::try_from(self.prices.len()).expect("bundled catalog price pool fits u16");
        assert!(id < u16::MAX, "u16::MAX is reserved for Option::None");
        self.prices.push(key);
        self.price_ids.insert(key, id);
        id
    }
}

fn req_f64(node: &Value, key: &str) -> f64 {
    node.get(key)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("missing float field {key}"))
}

fn tri_bool(value: Option<bool>) -> u8 {
    match value {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    }
}

fn pack_flags(model: &Value) -> u8 {
    let flag = |key: &str| u8::from(model.get(key).and_then(Value::as_bool).unwrap_or(false));
    let tri = |key: &str| tri_bool(model.get(key).and_then(Value::as_bool));
    flag("reasoning")
        | (flag("vision") << 1)
        | (flag("deprecated") << 2)
        | (tri("metered") << 3)
        | (tri("tools") << 5)
}

fn opt_u32(model: &Value, key: &str) -> u32 {
    let Some(raw) = model.get(key).and_then(Value::as_u64) else {
        return u32::MAX;
    };
    let value = u32::try_from(raw).unwrap_or_else(|_| panic!("{key} overflows u32: {raw}"));
    assert!(value < u32::MAX, "u32::MAX is reserved for Option::None");
    value
}

fn encode_model(pool: &mut Pool, id: &str, model: &Value) -> String {
    let (id_off, id_len) = pool.str_span(id);
    let (display_off, display_len) = pool.opt_str(model.get("display").and_then(Value::as_str));
    let (family_off, family_len) = pool.opt_str(model.get("family").and_then(Value::as_str));
    let context = opt_u32(model, "context");
    let release = opt_u32(model, "release");
    let price = pool.opt_price(model.get("price"));
    let flags = pack_flags(model);
    format!(
        "BundledModel{{context:{context},release:{release},id_off:{id_off},display_off:{display_off},family_off:{family_off},id_len:{id_len},display_len:{display_len},family_len:{family_len},price:{price},flags:{flags}}},"
    )
}

fn encode_provider(pool: &mut Pool, provider: &Value, models: &mut Vec<String>) -> String {
    let id = provider["id"].as_str().expect("provider id");
    let name = provider["name"].as_str().expect("provider name");
    let api = provider
        .get("api")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (id_off, id_len) = pool.str_span(id);
    let (name_off, name_len) = pool.str_span(name);
    let (api_off, api_len) = pool.str_span(api);
    let start = u16::try_from(models.len()).expect("bundled model count fits u16");
    let map = provider["models"].as_object().expect("provider models map");
    let mut entries: Vec<(&str, &Value)> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    for (model_id, meta) in entries {
        models.push(encode_model(pool, model_id, meta));
    }
    let count = u16::try_from(models.len()).expect("bundled model count fits u16") - start;
    format!(
        "BundledProvider{{id_off:{id_off},name_off:{name_off},api_off:{api_off},models_off:{start},models_len:{count},id_len:{id_len},name_len:{name_len},api_len:{api_len}}},"
    )
}

fn emit(root: &Value) -> String {
    assert_eq!(root["schema"].as_u64(), Some(3), "unexpected schema");
    let list = root["providers"].as_array().expect("providers array");
    let mut pool = Pool::default();
    let mut models = Vec::new();
    let mut providers = Vec::with_capacity(list.len());
    for provider in list {
        providers.push(encode_provider(&mut pool, provider, &mut models));
    }
    let mut out = String::with_capacity(196_608);
    let _ = writeln!(out, "static STRINGS: &str = {:?};", pool.strings);
    let _ = writeln!(out, "static PRICES: [[u64; 4]; {}] = [", pool.prices.len());
    for [i, o, c, w] in &pool.prices {
        let _ = writeln!(out, "[{i},{o},{c},{w}],");
    }
    let _ = writeln!(out, "];");
    let _ = writeln!(out, "static MODELS: [BundledModel; {}] = [", models.len());
    for row in &models {
        let _ = writeln!(out, "{row}");
    }
    let _ = writeln!(out, "];");
    let _ = writeln!(
        out,
        "static PROVIDERS: [BundledProvider; {}] = [",
        providers.len()
    );
    for row in &providers {
        let _ = writeln!(out, "{row}");
    }
    let _ = writeln!(out, "];");
    out
}
