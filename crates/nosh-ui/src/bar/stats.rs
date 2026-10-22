use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::SystemStats as Stats;

const CPU_ICON: char = '\u{f2db}';
const MEMORY_ICON: char = '\u{f0c9}';

const LEVEL_CLASSES: [&str; 4] = ["level-low", "level-medium", "level-high", "level-critical"];

pub struct SystemStats {
    root: gtk::Box,
    cpu: gtk::Label,
    memory: gtk::Label,
}

impl SystemStats {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("SystemStats");

        let cpu = metric_label("cpu-stat");
        let memory = metric_label("mem-stat");
        root.append(&cpu);
        root.append(&memory);

        let widget = Self { root, cpu, memory };
        widget.render(&Stats::default());
        widget
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn render(&self, stats: &Stats) {
        render_label(&self.cpu, CPU_ICON, stats.cpu_percent);
        render_label(&self.memory, MEMORY_ICON, stats.mem_percent);
    }
}

impl Default for SystemStats {
    fn default() -> Self {
        Self::new()
    }
}

fn metric_label(class: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class(class);
    label.set_xalign(1.0);
    label
}

fn render_label(label: &gtk::Label, icon: char, percent: u8) {
    let percent = percent.min(100);
    label.set_text(&format!("{icon} {percent}%"));

    let level = level_class(percent);
    for class in LEVEL_CLASSES {
        if class == level {
            label.add_css_class(class);
        } else {
            label.remove_css_class(class);
        }
    }
}

fn level_class(percent: u8) -> &'static str {
    match percent {
        0..=49 => "level-low",
        50..=69 => "level-medium",
        70..=89 => "level-high",
        _ => "level-critical",
    }
}

#[cfg(test)]
mod tests {
    use super::level_class;

    #[test]
    fn level_classes_follow_the_thresholds() {
        assert_eq!(level_class(0), "level-low");
        assert_eq!(level_class(49), "level-low");
        assert_eq!(level_class(50), "level-medium");
        assert_eq!(level_class(69), "level-medium");
        assert_eq!(level_class(70), "level-high");
        assert_eq!(level_class(89), "level-high");
        assert_eq!(level_class(90), "level-critical");
        assert_eq!(level_class(100), "level-critical");
    }
}
