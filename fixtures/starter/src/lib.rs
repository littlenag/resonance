//! Inventory core.

mod util;

pub use util::strings::normalize;

/// An item in the inventory.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub quantity: i64,
}

/// Records an item; rejects negative quantities.
pub fn record(items: &mut Vec<Item>, name: &str, quantity: i64) -> Result<(), String> {
    if quantity < 0 {
        return Err(format!("negative quantity for {name}"));
    }
    items.push(Item {
        name: normalize(name),
        quantity,
    });
    Ok(())
}

/// The total quantity of all items.
pub fn total(items: &[Item]) -> i64 {
    items.iter().map(|i| i.quantity).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_items() {
        let mut items = Vec::new();
        record(&mut items, "  Widget ", 3).unwrap();
        assert_eq!(items[0].name, "widget");
    }

    #[test]
    fn rejects_negative() {
        let mut items = Vec::new();
        assert!(record(&mut items, "x", -1).is_err());
    }
}
