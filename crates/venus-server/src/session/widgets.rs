use super::NotebookSession;
use std::collections::HashMap;
use venus_core::graph::CellId;
use venus_core::widgets::{WidgetDef, WidgetValue};

impl NotebookSession {
    /// Update a widget value for a cell.
    ///
    /// This stores the new value but does NOT trigger re-execution.
    /// The user must explicitly run the cell to see the effect.
    pub fn update_widget_value(&mut self, cell_id: CellId, widget_id: String, value: WidgetValue) {
        self.widget_values
            .entry(cell_id)
            .or_default()
            .insert(widget_id, value);
    }

    /// Get widget values for a cell.
    pub fn get_widget_values(&self, cell_id: CellId) -> HashMap<String, WidgetValue> {
        self.widget_values
            .get(&cell_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get ALL widget values from all cells, flattened into a single map.
    /// Widget IDs should be unique across the notebook.
    pub fn get_all_widget_values(&self) -> HashMap<String, WidgetValue> {
        let mut all_values = HashMap::new();
        for cell_widgets in self.widget_values.values() {
            for (widget_id, value) in cell_widgets {
                all_values.insert(widget_id.clone(), value.clone());
            }
        }
        all_values
    }

    /// Get widget definitions for a cell.
    pub fn get_widget_defs(&self, cell_id: CellId) -> Vec<WidgetDef> {
        self.widget_defs.get(&cell_id).cloned().unwrap_or_default()
    }

    /// Store widget definitions from cell execution.
    pub(super) fn store_widget_defs(&mut self, cell_id: CellId, widgets: Vec<WidgetDef>) {
        if widgets.is_empty() {
            self.widget_defs.remove(&cell_id);
        } else {
            self.widget_defs.insert(cell_id, widgets);
        }
    }
}
