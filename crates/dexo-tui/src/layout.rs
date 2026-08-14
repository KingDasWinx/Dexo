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
        let mode = if area.width >= 120 && area.height >= 35 {
            LayoutMode::Full
        } else if area.width >= 80 && area.height >= 24 {
            LayoutMode::Reduced
        } else {
            LayoutMode::Compact
        };
        match mode {
            LayoutMode::Full => full_layout(area),
            LayoutMode::Reduced => reduced_layout(area),
            LayoutMode::Compact => compact_layout(area),
        }
    }
}

fn full_layout(area: Rect) -> LayoutPlan {
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
    let explorer_w = body.width.saturating_mul(22) / 100;
    let inspector_w = body.width.saturating_mul(22) / 100;
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
    let results_h = center.height.saturating_mul(35) / 100;
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

fn reduced_layout(area: Rect) -> LayoutPlan {
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
    let explorer_w = body.width.saturating_mul(24) / 100;
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
}
