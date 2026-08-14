use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutMode {
    Full,
    Reduced,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutPlan {
    pub mode: LayoutMode,
    pub context: Rect,
    pub explorer: Rect,
    pub tabs: Rect,
    pub content: Rect,
    pub results: Rect,
    pub inspector: Rect,
    pub status: Rect,
}

impl LayoutPlan {
    pub fn for_area(area: Rect) -> Self {
        Self::for_area_with(area, None)
    }

    pub fn for_area_with(area: Rect, panes: Option<&PaneLayout>) -> Self {
        let mode = if area.width >= 120 && area.height >= 35 {
            LayoutMode::Full
        } else if area.width >= 80 && area.height >= 24 {
            LayoutMode::Reduced
        } else {
            LayoutMode::Compact
        };
        match mode {
            LayoutMode::Full => full_layout(area, panes),
            LayoutMode::Reduced => reduced_layout(area, panes),
            LayoutMode::Compact => compact_layout(area),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneLayout {
    pub explorer_visible: bool,
    pub inspector_visible: bool,
    pub results_visible: bool,
    pub explorer_width: u16,
    pub inspector_width: u16,
    pub results_height: u16,
}

impl PaneLayout {
    pub fn clamp(mut self, width: u16, height: u16) -> Self {
        let max_side = width.saturating_div(2).max(8);
        let max_results = height.saturating_sub(6).max(3);
        self.explorer_width = self.explorer_width.min(max_side).max(8);
        self.inspector_width = self.inspector_width.min(max_side).max(8);
        self.results_height = self.results_height.min(max_results).max(3);
        if width < 80 {
            self.inspector_visible = false;
        }
        if width < 60 || height < 24 {
            self.explorer_visible = false;
            self.inspector_visible = false;
            self.results_visible = false;
        }
        self
    }
}

fn full_layout(area: Rect, panes: Option<&PaneLayout>) -> LayoutPlan {
    let context_h = 1.min(area.height);
    let status_h = 1.min(area.height.saturating_sub(context_h));
    let body_h = area.height.saturating_sub(context_h + status_h);
    let context = Rect::new(area.x, area.y, area.width, context_h);
    let status = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(status_h)),
        area.width,
        status_h,
    );
    let body = Rect::new(area.x, area.y.saturating_add(context_h), area.width, body_h);
    let explorer_w = pane_width(
        panes,
        |p| p.explorer_visible,
        |p| p.explorer_width,
        body.width,
        22,
    );
    let inspector_w = pane_width(
        panes,
        |p| p.inspector_visible,
        |p| p.inspector_width,
        body.width,
        22,
    );
    let center_w = body.width.saturating_sub(explorer_w + inspector_w);
    let explorer = Rect::new(body.x, body.y, explorer_w, body.height);
    let center = Rect::new(
        body.x.saturating_add(explorer_w),
        body.y,
        center_w,
        body.height,
    );
    let inspector = Rect::new(
        body.x.saturating_add(explorer_w + center_w),
        body.y,
        inspector_w,
        body.height,
    );
    let tabs_h = 1.min(center.height);
    let results_h = match panes {
        Some(p) if !p.results_visible => 0,
        Some(p) => p.results_height.min(center.height.saturating_sub(2)),
        None => center.height.saturating_mul(35) / 100,
    };
    let content_h = center.height.saturating_sub(tabs_h + results_h);
    let tabs = Rect::new(center.x, center.y, center.width, tabs_h);
    let content = Rect::new(
        center.x,
        center.y.saturating_add(tabs_h),
        center.width,
        content_h,
    );
    let results = Rect::new(
        center.x,
        center.y.saturating_add(tabs_h + content_h),
        center.width,
        results_h,
    );
    LayoutPlan {
        mode: LayoutMode::Full,
        context,
        explorer,
        tabs,
        content,
        results,
        inspector,
        status,
    }
}

fn pane_width(
    panes: Option<&PaneLayout>,
    visible: impl Fn(&PaneLayout) -> bool,
    width: impl Fn(&PaneLayout) -> u16,
    body_width: u16,
    percent: u16,
) -> u16 {
    match panes {
        Some(p) if !visible(p) => 0,
        Some(p) => width(p).min(body_width / 2),
        None => body_width.saturating_mul(percent) / 100,
    }
}

fn reduced_layout(area: Rect, panes: Option<&PaneLayout>) -> LayoutPlan {
    let context_h = 1.min(area.height);
    let status_h = 1.min(area.height.saturating_sub(context_h));
    let body_h = area.height.saturating_sub(context_h + status_h);
    let context = Rect::new(area.x, area.y, area.width, context_h);
    let status = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(status_h)),
        area.width,
        status_h,
    );
    let body = Rect::new(area.x, area.y.saturating_add(context_h), area.width, body_h);
    let explorer_w = pane_width(
        panes,
        |p| p.explorer_visible,
        |p| p.explorer_width,
        body.width,
        24,
    );
    let center_w = body.width.saturating_sub(explorer_w);
    let explorer = Rect::new(body.x, body.y, explorer_w, body.height);
    let center = Rect::new(
        body.x.saturating_add(explorer_w),
        body.y,
        center_w,
        body.height,
    );
    let tabs_h = 1.min(center.height);
    let results_h = center.height.saturating_mul(30) / 100;
    let content_h = center.height.saturating_sub(tabs_h + results_h);
    let tabs = Rect::new(center.x, center.y, center.width, tabs_h);
    let content = Rect::new(
        center.x,
        center.y.saturating_add(tabs_h),
        center.width,
        content_h,
    );
    let results = Rect::new(
        center.x,
        center.y.saturating_add(tabs_h + content_h),
        center.width,
        results_h,
    );
    // ponytail: reduced mode hides the inspector pane; restore a split when users persist pane sizes
    LayoutPlan {
        mode: LayoutMode::Reduced,
        context,
        explorer,
        tabs,
        content,
        results,
        inspector: Rect::new(0, 0, 0, 0),
        status,
    }
}

fn compact_layout(area: Rect) -> LayoutPlan {
    let context_h = 1.min(area.height);
    let status_h = 1.min(area.height.saturating_sub(context_h));
    let body_h = area.height.saturating_sub(context_h + status_h);
    let context = Rect::new(area.x, area.y, area.width, context_h);
    let status = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(status_h)),
        area.width,
        status_h,
    );
    let body = Rect::new(area.x, area.y.saturating_add(context_h), area.width, body_h);
    LayoutPlan {
        mode: LayoutMode::Compact,
        context,
        explorer: Rect::new(0, 0, 0, 0),
        tabs: Rect::new(0, 0, 0, 0),
        content: body,
        results: Rect::new(0, 0, 0, 0),
        inspector: Rect::new(0, 0, 0, 0),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::{LayoutMode, LayoutPlan};
    use ratatui::layout::Rect;

    #[test]
    fn layout_matches_terminal() {
        for (width, height, expected) in [
            (160, 50, LayoutMode::Full),
            (100, 30, LayoutMode::Reduced),
            (60, 20, LayoutMode::Compact),
        ] {
            assert_eq!(
                LayoutPlan::for_area(Rect::new(0, 0, width, height)).mode,
                expected
            );
        }
    }

    #[test]
    fn restored_sizes_are_clamped_to_terminal() {
        use super::PaneLayout;
        let huge = PaneLayout {
            explorer_visible: true,
            inspector_visible: true,
            results_visible: true,
            explorer_width: 400,
            inspector_width: 400,
            results_height: 400,
        }
        .clamp(160, 50);
        let plan = LayoutPlan::for_area_with(Rect::new(0, 0, 160, 50), Some(&huge));
        assert!(plan.explorer.width <= 80);
        assert!(plan.inspector.width <= 80);
        assert!(plan.results.height <= 44);
        let compact = huge.clamp(50, 18);
        assert!(!compact.explorer_visible);
        let compact_plan = LayoutPlan::for_area_with(Rect::new(0, 0, 50, 18), Some(&compact));
        assert_eq!(compact_plan.mode, LayoutMode::Compact);
        assert_eq!(compact_plan.explorer.width, 0);
    }
}
