//! Deterministic declarations derived from the serialized Rust contracts.

use std::collections::{BTreeMap, HashSet};
use ts_rs::{Config, TypeVisitor, TS};

pub struct TypeScript {
    config: Config,
    visited: HashSet<&'static str>,
    declarations: BTreeMap<String, String>,
    collisions: Vec<String>,
}

impl Default for TypeScript {
    fn default() -> Self {
        Self {
            // IPC uses JSON numbers. Domain commands validate integer ranges;
            // mapping them to bigint would describe a different wire format.
            config: Config::new().with_large_int("number"),
            visited: HashSet::new(),
            declarations: BTreeMap::new(),
            collisions: Vec::new(),
        }
    }
}

impl TypeScript {
    pub fn add<T: TS + 'static + ?Sized>(&mut self) {
        self.visit::<T>();
    }

    pub fn finish(self) -> Result<String, String> {
        if !self.collisions.is_empty() {
            return Err(format!(
                "Conflicting exported type names: {}",
                self.collisions.join(", ")
            ));
        }
        let body = self
            .declarations
            .into_values()
            .collect::<Vec<_>>()
            .join("\n\n");
        let source = format!(
            "// Derived from Rust wire contracts. Run the binding export test to update.\n\n{body}"
        );
        Ok(source
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n")
    }
}

impl TypeVisitor for TypeScript {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if !self.visited.insert(std::any::type_name::<T>()) {
            return;
        }
        if T::output_path().is_some() {
            let name = T::name(&self.config);
            let declaration = format!("export {}", T::decl(&self.config));
            if let Some(previous) = self.declarations.insert(name.clone(), declaration.clone()) {
                if previous != declaration {
                    self.collisions.push(name);
                }
            }
        }
        T::visit_dependencies(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, TS)]
    struct Example {
        #[serde(rename = "wireName")]
        name: String,
        nullable: Option<f64>,
    }

    #[test]
    fn follows_serialization_names_and_nullability() {
        let mut declarations = TypeScript::default();
        declarations.add::<Example>();
        declarations.add::<Example>();
        let source = declarations.finish().expect("contracts");
        assert!(source.contains("wireName: string"));
        assert!(source.contains("nullable: number | null"));
        assert_eq!(source.matches("export type Example").count(), 1);
        let json = serde_json::to_value(Example {
            name: "item".into(),
            nullable: None,
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({"wireName":"item", "nullable":null})
        );
    }
}
