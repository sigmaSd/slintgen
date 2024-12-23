use std::{collections::HashSet, fs};
use tree_sitter::{Parser, TreeCursor};

fn main() {
    // Initialize Tree-sitter parser
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_slint::LANGUAGE.into())
        .expect("Error loading Slint grammar");

    // Read and parse the Slint file
    let source_code = fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let tree = parser.parse(&source_code, None).unwrap();
    let root_node = tree.root_node();

    // Convert source code to bytes for utf8_text extraction
    let source_bytes = source_code.as_bytes();

    // Traverse and process nodes using TreeCursor
    let mut cursor = root_node.walk();
    let mut seen_types = HashSet::new();
    println!("// deno-lint-ignore-file no-explicit-any");
    process_node(&mut cursor, source_bytes, &mut seen_types);
}

fn process_node(cursor: &mut TreeCursor, source_bytes: &[u8], seen_types: &mut HashSet<String>) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        match kind {
            "struct_definition" => {
                process_struct(cursor, source_bytes, seen_types);
            }
            "export" => {
                if let Some(sibling) = node.next_sibling() {
                    if sibling.kind() == "component_definition" {
                        cursor.goto_next_sibling();
                        process_component(cursor, source_bytes, seen_types);
                    }
                }
            }
            _ => {}
        }

        // Traverse child nodes
        if cursor.goto_first_child() {
            process_node(cursor, source_bytes, seen_types);
            cursor.goto_parent();
        }

        // Move to the next sibling node
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn process_struct(cursor: &mut TreeCursor, source_bytes: &[u8], seen_types: &mut HashSet<String>) {
    // Extract struct name
    let struct_name = find_child_by_kind(cursor, "user_type_identifier", source_bytes)
        .unwrap_or("UnnamedStruct".to_string());
    seen_types.insert(struct_name.clone());

    // Extract fields
    let fields = extract_struct_fields(cursor, source_bytes, seen_types);

    // Generate TypeScript interface
    println!("export interface {} {{", struct_name);
    for (name, field_type) in fields {
        println!(
            "  {}: {};",
            map_name(name),
            map_type(field_type, seen_types)
        );
    }
    println!("}}");
}

fn process_component(
    cursor: &mut TreeCursor,
    source_bytes: &[u8],
    seen_types: &mut HashSet<String>,
) {
    // Extract component name
    let component_name = find_child_by_kind(cursor, "user_type_identifier", source_bytes)
        .unwrap_or("UnnamedComponent".to_string());

    // Extract properties
    let (properties, callbacks) = extract_component_properties(cursor, source_bytes, seen_types);

    // Generate TypeScript interface
    println!("export interface {} {{", component_name);
    for (name, prop_type) in properties {
        println!("  {}: {};", map_name(name), map_type(prop_type, seen_types));
    }
    for callback in callbacks {
        print!(
            "  {}: ({})",
            map_name(callback.name),
            callback
                .args
                .iter()
                .enumerate()
                .map(|(index, x)| format!(
                    "{}: {}",
                    format!("arg{}", index),
                    map_type(x.to_string(), seen_types)
                ))
                .collect::<Vec<String>>()
                .join(", "),
        );
        if let Some(return_type) = callback.return_type {
            println!(" => {};", map_type(return_type, seen_types));
        } else {
            println!(" => void;")
        }
    }
    // add a run function
    println!("  run: () => Promise<void>;");
    println!("}}");
}

fn extract_component_properties(
    cursor: &mut TreeCursor,
    source_bytes: &[u8],
    _seen_types: &mut HashSet<String>,
) -> (Vec<(String, String)>, Vec<Callback>) {
    let mut properties = Vec::new();
    let mut callbacks = Vec::new();
    let node = cursor.node();

    if node.kind() == "component_definition" {
        let mut prop_cursor = cursor.clone();

        if prop_cursor.goto_first_child() {
            loop {
                if prop_cursor.node().kind() == "block" {
                    if prop_cursor.goto_first_child() {
                        loop {
                            if prop_cursor.node().kind() == "callback" {
                                let callback_name = prop_cursor
                                    .node()
                                    .child_by_field_name("name")
                                    .unwrap()
                                    .utf8_text(source_bytes)
                                    .unwrap()
                                    .to_string();
                                let callback_return = prop_cursor
                                    .node()
                                    .child_by_field_name("return_type")
                                    .map(|x| x.utf8_text(source_bytes).unwrap().to_string());
                                let callback_args = prop_cursor
                                    .node()
                                    // clone so we don't modify the cursor
                                    .children_by_field_name("arguments", &mut prop_cursor.clone())
                                    .map(|x| x.utf8_text(source_bytes).unwrap().to_string())
                                    .filter(|x| x != ",")
                                    .collect();
                                callbacks.push(Callback {
                                    name: callback_name,
                                    args: callback_args,
                                    return_type: callback_return,
                                });
                            }
                            if prop_cursor.node().kind() == "property" {
                                let prop_name = prop_cursor
                                    .node()
                                    .child_by_field_name("name")
                                    .unwrap()
                                    .utf8_text(source_bytes)
                                    .unwrap()
                                    .to_string();
                                let prop_type = prop_cursor
                                    .node()
                                    .child_by_field_name("type")
                                    .unwrap()
                                    .utf8_text(source_bytes)
                                    .unwrap()
                                    .to_string();
                                properties.push((prop_name, prop_type));
                            }
                            if prop_cursor.node().kind() == "property_assignment" {
                                let prop = prop_cursor
                                    .node()
                                    .child_by_field_name("property")
                                    .unwrap()
                                    .utf8_text(source_bytes)
                                    .unwrap()
                                    .to_string();
                                let prop_value = prop_cursor
                                    .node()
                                    .child_by_field_name("value")
                                    .unwrap()
                                    .utf8_text(source_bytes)
                                    .unwrap()
                                    .to_string();
                                properties.push((prop, prop_value));
                            }
                            if !prop_cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                }
                if !prop_cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    (properties, callbacks)
}

fn extract_struct_fields(
    cursor: &mut TreeCursor,
    source_bytes: &[u8],
    _seen_types: &mut HashSet<String>,
) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let node = cursor.node();

    if node.kind() == "struct_definition" {
        let mut field_cursor = cursor.clone();

        if field_cursor.goto_first_child() {
            loop {
                let child_node = field_cursor.node();
                if child_node.kind() == "struct_block" {
                    let mut field_names = vec![];
                    let mut field_types = vec![];
                    for field_name in child_node.children_by_field_name("name", &mut field_cursor) {
                        field_names.push(field_name.utf8_text(source_bytes).unwrap().to_string());
                    }
                    for field_type in child_node.children_by_field_name("type", &mut field_cursor) {
                        field_types.push(field_type.utf8_text(source_bytes).unwrap().to_string());
                    }
                    fields = field_names
                        .into_iter()
                        .zip(field_types.into_iter())
                        .collect();
                }
                if !field_cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fields
}

fn find_child_by_kind(cursor: &TreeCursor, kind: &str, source_bytes: &[u8]) -> Option<String> {
    let mut child_cursor = cursor.clone();
    if child_cursor.goto_first_child() {
        loop {
            let node = child_cursor.node();
            if node.kind() == kind {
                return Some(node.utf8_text(source_bytes).unwrap().to_string());
            }
            if !child_cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

fn map_type(slint_type: String, seen_types: &HashSet<String>) -> String {
    match slint_type.as_str() {
        "int" => "number".to_string(),
        "bool" => "boolean".to_string(),
        "string" => "string".to_string(),
        t if t.starts_with("[") && t.ends_with("]") => {
            // array
            format!("{}[]", map_type(t[1..t.len() - 1].to_string(), seen_types))
        }
        t if seen_types.contains(t) => {
            // type alias
            t.to_string()
        }
        _ => "any".to_string(),
    }
}

fn map_name(slint_name: String) -> String {
    slint_name.replace("-", "_")
}

struct Callback {
    name: String,
    args: Vec<String>,
    return_type: Option<String>,
}
