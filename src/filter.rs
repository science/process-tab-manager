/// Decides whether a window's WM_CLASS should be managed by PTM.
pub struct Filter {
    classes_lower: Vec<String>,
}

impl Filter {
    pub fn new(classes: Vec<String>) -> Self {
        Self {
            classes_lower: classes.iter().map(|c| c.to_lowercase()).collect(),
        }
    }

    pub fn matches(&self, wm_class: &str) -> bool {
        let lower = wm_class.to_lowercase();
        self.classes_lower.iter().any(|c| c == &lower)
    }
}
