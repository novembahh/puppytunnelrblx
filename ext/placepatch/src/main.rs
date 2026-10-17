use rbx_types::Ref;
use rbx_dom_weak::{InstanceBuilder, WeakDom};
use std::fs::File;
use std::io::BufReader;

fn find_service(dom: &WeakDom, class: &str) -> Option<Ref> {
    let root = dom.root_ref();
    let root_inst = dom.get_by_ref(root)?;
    for child in root_inst.children() {
        if let Some(inst) = dom.get_by_ref(*child) {
            if inst.class.as_str() == class {
                return Some(*child);
            }
        }
    }
    None
}

fn main() -> anyhow::Result<()> {
    let place_path = std::env::args().nth(1).expect("usage: placepatch <place.rbxl> <script.lua> [--count-only]");
    let script_path = std::env::args().nth(2).expect("usage: placepatch <place.rbxl> <script.lua> [--count-only]");
    let count_only = std::env::args().any(|a| a == "--count-only");

    let input = BufReader::new(File::open(&place_path)?);
    let mut dom: WeakDom = rbx_binary::from_reader(input)?;

    if count_only {
        {
            let root = dom.root_ref();
            if let Some(r) = dom.get_by_ref(root) {
                println!("ROOT class={} name={}", r.class, r.name);
                let mut kids: Vec<String> = Vec::new();
                for c in r.children().iter().take(40) {
                    if let Some(k) = dom.get_by_ref(*c) {
                        kids.push(format!("{}/{}", k.class, k.name));
                    }
                }
                println!("ROOT kids({}): {}", r.children().len(), kids.join(", "));
            }
        }
        let mut total = 0;
        let mut by_class = std::collections::HashMap::new();
        let mut stack = vec![dom.root_ref()];
        while let Some(id) = stack.pop() {
            if let Some(inst) = dom.get_by_ref(id) {
                total += 1;
                *by_class.entry(inst.class.to_string()).or_insert(0) += 1;
                stack.extend(inst.children().iter().cloned());
            }
        }
        println!("total: {}", total);
        let mut classes: Vec<_> = by_class.into_iter().collect();
        classes.sort_by_key(|(_, n)| -(*n));
        for (c, n) in classes.iter() {
            println!("  {}: {}", c, n);
        }
        let mut stack = vec![dom.root_ref()];
        while let Some(id) = stack.pop() {
            if let Some(inst) = dom.get_by_ref(id) {
                if inst.name.as_str() == "PuppyMaint" {
                    let src_len = match inst.properties.get(&"Source".into()) {
                        Some(v) => format!("{:?}", v).len(),
                        None => 0,
                    };
                    let parent_name = dom
                        .get_by_ref(inst.parent())
                        .map(|p| format!("{}/{}", p.class, p.name))
                        .unwrap_or_else(|| "?".to_string());
                    println!("PUPPYMAINT parent={} source_debug_len={}", parent_name, src_len);
                }
                stack.extend(inst.children().iter().cloned());
            }
        }
        return Ok(());
    }

    let source = std::fs::read_to_string(&script_path)?;

    let sss = find_service(&dom, "ServerScriptService").expect("no ServerScriptService in place");

    let mut old = Vec::new();
    if let Some(sss_inst) = dom.get_by_ref(sss) {
        for child in sss_inst.children() {
            if let Some(inst) = dom.get_by_ref(*child) {
                if inst.name.as_str() == "PuppyMaint" {
                    old.push(*child);
                }
            }
        }
    }
    for id in old {
        dom.destroy(id);
    }

    let script = InstanceBuilder::new("Script")
        .with_name("PuppyMaint")
        .with_property("Source", source.as_str());
    dom.insert(sss, script);

    let top: Vec<Ref> = dom
        .get_by_ref(dom.root_ref())
        .map(|r| r.children().to_vec())
        .unwrap_or_default();
    let out = File::create(&place_path)?;
    rbx_binary::to_writer(out, &dom, &top)?;

    println!("patched {} (PuppyMaint installed)", place_path);
    Ok(())
}
