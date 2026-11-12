//! Pointer activation policy for the non-activating transparent reader window.

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn should_reveal(bounds: Bounds, pointer: (f64, f64), ready: bool, suspended: bool) -> bool {
    ready
        && !suspended
        && [
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            pointer.0,
            pointer.1,
        ]
        .iter()
        .all(|v| v.is_finite())
        && bounds.width > 0.
        && bounds.height > 0.
        && pointer.0 >= bounds.x
        && pointer.0 < bounds.x + bounds.width
        && pointer.1 >= bounds.y
        && pointer.1 < bounds.y + bounds.height
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub enum ResizeEdge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedHoverGeometry {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}
impl Default for SavedHoverGeometry {
    fn default() -> Self {
        Self {
            width: 620.,
            height: 190.,
            x: None,
            y: None,
        }
    }
}
impl SavedHoverGeometry {
    pub fn is_valid(&self) -> bool {
        self.width.is_finite()
            && self.height.is_finite()
            && (280. ..=8192.).contains(&self.width)
            && (90. ..=8192.).contains(&self.height)
    }

    /// Legacy files have no position. Invalid/offscreen positions fall back to
    /// centering, while keeping the saved size within the current screen.
    pub fn restore(
        &self,
        monitors: &[super::window_state::MonitorRect],
    ) -> (tauri::LogicalSize<f64>, Option<tauri::LogicalPosition<f64>>) {
        let position = self
            .x
            .zip(self.y)
            .filter(|(x, y)| x.is_finite() && y.is_finite());
        let (x, y) = position.unwrap_or((f64::NAN, f64::NAN));
        super::window_state::sanitize_geometry(self.width, self.height, x, y, monitors, (280., 90.))
    }
}

/// Cursor delta and bounds use physical pixels; constraints use logical pixels.
/// Always derive from the initial bounds, keeping the opposite edge fixed.
pub fn resized_bounds(b: Bounds, edge: ResizeEdge, delta: (f64, f64), scale: f64) -> Bounds {
    let west = matches!(
        edge,
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest
    );
    let east = matches!(
        edge,
        ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast
    );
    let north = matches!(
        edge,
        ResizeEdge::North | ResizeEdge::NorthWest | ResizeEdge::NorthEast
    );
    let south = matches!(
        edge,
        ResizeEdge::South | ResizeEdge::SouthWest | ResizeEdge::SouthEast
    );
    let width = if west || east {
        (b.width + if west { -delta.0 } else { delta.0 }).clamp(280. * scale, 8192. * scale)
    } else {
        b.width
    };
    let height = if north || south {
        (b.height + if north { -delta.1 } else { delta.1 }).clamp(90. * scale, 8192. * scale)
    } else {
        b.height
    };
    Bounds {
        x: if west { b.x + b.width - width } else { b.x },
        y: if north { b.y + b.height - height } else { b.y },
        width,
        height,
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn saved_geometry_retains_position_across_serialization() {
        let json = serde_json::json!({"width": 440., "height": 235., "x": -820., "y": 145.});
        let saved: SavedHoverGeometry = serde_json::from_value(json.clone()).unwrap();
        assert!(saved.is_valid());
        assert_eq!(serde_json::to_value(saved).unwrap(), json);
    }

    #[test]
    fn legacy_size_only_file_keeps_dimensions_and_centers() {
        let saved: SavedHoverGeometry =
            serde_json::from_str(r#"{"width":440,"height":235}"#).unwrap();
        let (size, position) = saved.restore(&[crate::app::window_state::MonitorRect {
            x: 0.,
            y: 0.,
            width: 1440.,
            height: 900.,
        }]);
        assert_eq!((size.width, size.height), (440., 235.));
        assert!(position.is_none());
    }

    #[test]
    fn restore_preserves_negative_monitor_position_but_recovers_disconnected_monitor() {
        use crate::app::window_state::MonitorRect;
        let primary = MonitorRect {
            x: 0.,
            y: 0.,
            width: 1440.,
            height: 900.,
        };
        let secondary = MonitorRect {
            x: -1920.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        };
        let saved: SavedHoverGeometry = serde_json::from_value(serde_json::json!({
            "width": 1800., "height": 950., "x": -1900., "y": 100.
        }))
        .unwrap();
        let (size, position) = saved.restore(&[primary, secondary]);
        assert_eq!((size.width, size.height), (1800., 950.));
        let position = position.unwrap();
        assert_eq!((position.x, position.y), (-1900., 100.));
        let (size, position) = saved.restore(&[primary]);
        assert_eq!((size.width, size.height), (1440., 900.));
        assert!(position.is_none());
    }

    #[test]
    fn incomplete_or_nonfinite_position_centers_without_losing_size() {
        let monitor = crate::app::window_state::MonitorRect {
            x: 0.,
            y: 0.,
            width: 1440.,
            height: 900.,
        };
        for (x, y) in [
            (Some(100.), None),
            (Some(f64::NAN), Some(100.)),
            (None, Some(100.)),
        ] {
            let saved = SavedHoverGeometry {
                width: 440.,
                height: 235.,
                x,
                y,
            };
            let (size, position) = saved.restore(&[monitor]);
            assert_eq!((size.width, size.height), (440., 235.));
            assert!(position.is_none());
        }
    }

    #[test]
    fn opposite_edges_remain_fixed_in_all_eight_directions() {
        let b = Bounds {
            x: -600.,
            y: 120.,
            width: 620.,
            height: 190.,
        };
        for (direction, expected) in [
            (ResizeEdge::East, Bounds { width: 700., ..b }),
            (
                ResizeEdge::West,
                Bounds {
                    x: -520.,
                    width: 540.,
                    ..b
                },
            ),
            (
                ResizeEdge::North,
                Bounds {
                    y: 160.,
                    height: 150.,
                    ..b
                },
            ),
            (ResizeEdge::South, Bounds { height: 230., ..b }),
            (
                ResizeEdge::NorthEast,
                Bounds {
                    y: 160.,
                    width: 700.,
                    height: 150.,
                    ..b
                },
            ),
            (
                ResizeEdge::NorthWest,
                Bounds {
                    x: -520.,
                    y: 160.,
                    width: 540.,
                    height: 150.,
                },
            ),
            (
                ResizeEdge::SouthEast,
                Bounds {
                    width: 700.,
                    height: 230.,
                    ..b
                },
            ),
            (
                ResizeEdge::SouthWest,
                Bounds {
                    x: -520.,
                    width: 540.,
                    height: 230.,
                    ..b
                },
            ),
        ] {
            assert_eq!(resized_bounds(b, direction, (80., 40.), 1.), expected);
        }
    }
    #[test]
    fn minimum_size_preserves_anchor_on_retina_displays() {
        let b = Bounds {
            x: 100.,
            y: 200.,
            width: 1240.,
            height: 380.,
        };
        assert_eq!(
            resized_bounds(b, ResizeEdge::NorthWest, (2000., 2000.), 2.),
            Bounds {
                x: 780.,
                y: 400.,
                width: 560.,
                height: 180.
            }
        );
        assert_eq!(
            resized_bounds(b, ResizeEdge::SouthEast, (-2000., -2000.), 2.),
            Bounds {
                width: 560.,
                height: 180.,
                ..b
            }
        );
    }
    #[test]
    fn invalid_saved_geometry_does_not_reopen_an_unusable_window() {
        assert!(SavedHoverGeometry {
            width: 800.,
            height: 300.,
            ..Default::default()
        }
        .is_valid());
        for width in [0., 20., f64::NAN, f64::INFINITY, 30000.] {
            assert!(!SavedHoverGeometry {
                width,
                height: 190.,
                ..Default::default()
            }
            .is_valid());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_reveals_only_inside_ready_window_and_hides_on_exit() {
        let b = Bounds {
            x: -600.,
            y: 120.,
            width: 560.,
            height: 180.,
        };
        assert!(should_reveal(b, (-599., 121.), true, false));
        assert!(should_reveal(b, (-600., 120.), true, false));
        for p in [(-601., 150.), (-40., 150.), (-100., 300.), (-100., 119.)] {
            assert!(!should_reveal(b, p, true, false));
        }
        assert!(!should_reveal(b, (-300., 150.), false, false));
        assert!(!should_reveal(b, (-300., 150.), true, true));
    }
    #[test]
    fn invalid_geometry_or_pointer_never_reveals_content() {
        let b = Bounds {
            x: 0.,
            y: 0.,
            width: 500.,
            height: 160.,
        };
        for p in [(f64::NAN, 20.), (20., f64::INFINITY)] {
            assert!(!should_reveal(b, p, true, false));
        }
        assert!(!should_reveal(
            Bounds { width: 0., ..b },
            (0., 0.),
            true,
            false
        ));
    }
}
