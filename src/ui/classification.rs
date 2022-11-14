use bevy::prelude::*;
use bevy_egui::EguiContext;
use egui::{color_picker::show_color, ScrollArea};
use imc_rs::{Acquisitions, ChannelIdentifier};
use std::collections::HashMap;

use crate::{
    annotation::Annotation,
    camera::FieldOfView,
    imc::{self, Acquisition, ClassifierOutput, IMCDataset, IMCEvent},
};

use super::{DataEvent, UiEvent};

pub struct ClassificationUiPlugin;

impl Plugin for ClassificationUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_startup_system(setup)
            .add_system(ui_classification_window);
    }
}

#[derive(Debug, PartialEq, Clone)]
enum ClassificationTarget {
    FieldOfView,
    WholeImage,
}

#[derive(Component)]
struct ClassificationWindow {
    target: ClassificationTarget,

    auto_update: bool,

    acquisitions: HashMap<String, bool>,
    channels: HashMap<String, bool>,
    annotations: HashMap<Entity, bool>,
}

fn setup(mut commands: Commands) {
    commands.spawn(ClassificationWindow {
        target: ClassificationTarget::FieldOfView,
        auto_update: false,
        acquisitions: HashMap::new(),
        channels: HashMap::new(),
        annotations: HashMap::new(),
    });
}

fn ui_classification_window(
    mut egui_ctx: ResMut<EguiContext>,
    mut q_window: Query<&mut ClassificationWindow>,
    q_imc: Query<&IMCDataset>,
    q_acquisition: Query<(&Transform, With<Acquisition>)>,
    q_annotation: Query<(Entity, &Annotation)>,
    q_fov: Query<&FieldOfView>,
    mut ui_events: EventWriter<UiEvent>,
) {
    for mut window in q_window.iter_mut() {
        egui::Window::new("Classification")
            //.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(egui_ctx.ctx_mut(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Classify ");

                    ui.selectable_value(
                        &mut window.target,
                        ClassificationTarget::FieldOfView,
                        "Field of view",
                    );
                    ui.selectable_value(
                        &mut window.target,
                        ClassificationTarget::WholeImage,
                        "Whole image",
                    );
                });

                if window.target == ClassificationTarget::WholeImage {
                    ui.label("Acquisitions");

                    egui::Grid::new("acquisitions_grid")
                        .num_columns(3)
                        .show(ui, |ui| {
                            for imc in q_imc.iter() {
                                for (index, acquisition) in imc.acquisitions().iter().enumerate() {
                                    match window
                                        .acquisitions
                                        .entry(acquisition.description().to_string())
                                    {
                                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                                            ui.checkbox(entry.get_mut(), acquisition.description());
                                        }
                                        std::collections::hash_map::Entry::Vacant(entry) => {
                                            ui.checkbox(
                                                entry.insert(false),
                                                acquisition.description(),
                                            );
                                        }
                                    };

                                    if index % 3 == 2 {
                                        ui.end_row();
                                    }
                                }
                            }
                        });
                } else {
                    ui.checkbox(&mut window.auto_update, "Auto update");
                }

                ui.separator();

                let mut acquisitions = Vec::new();

                let fov = q_fov.single();
                let fov = imc_rs::BoundingBox {
                    min_x: fov.top_left.x as f64,
                    min_y: fov.bottom_right.y as f64,
                    width: (fov.bottom_right.x - fov.top_left.x) as f64,
                    height: (fov.top_left.y - fov.bottom_right.y) as f64,
                };

                // Check which acquisitions are within the field of view, and combine their channels
                for imc in q_imc.iter() {
                    match window.target {
                        ClassificationTarget::FieldOfView => {
                            acquisitions.extend(imc.acquisitions_in(&fov).iter());
                        }
                        ClassificationTarget::WholeImage => {
                            for acquisition in imc.acquisitions() {
                                if let Some(true) =
                                    window.acquisitions.get(acquisition.description())
                                {
                                    acquisitions.push(acquisition);
                                }
                            }
                        }
                    }
                }

                let channels = acquisitions.channels();
                let id = ui.make_persistent_id("header_for_channels");
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    true,
                )
                .show_header(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Channels");

                        ui.button("All");
                        ui.button("None");
                    });
                })
                .body(|ui| {
                    ui.set_max_height(100.0);

                    ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show_viewport(ui, |ui, viewport| {
                            // List all channels
                            egui::Grid::new("channels_grid")
                                .num_columns(3)
                                .show(ui, |ui| {
                                    for (index, channel) in channels.iter().enumerate() {
                                        match window.channels.entry(channel.label().to_string()) {
                                            std::collections::hash_map::Entry::Occupied(
                                                mut entry,
                                            ) => {
                                                ui.checkbox(entry.get_mut(), channel.label());
                                            }
                                            std::collections::hash_map::Entry::Vacant(entry) => {
                                                ui.checkbox(entry.insert(false), channel.label());
                                            }
                                        }

                                        if index % 3 == 2 {
                                            ui.end_row();
                                        }
                                    }
                                });
                        });
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Labels");

                    if ui.button("All").clicked() {
                        for (entity, _) in q_annotation.iter() {
                            match window.annotations.entry(entity) {
                                std::collections::hash_map::Entry::Occupied(mut entry) => {
                                    *entry.get_mut() = true;
                                }
                                std::collections::hash_map::Entry::Vacant(entry) => {
                                    entry.insert(true);
                                }
                            }
                        }
                    }

                    if ui.button("None").clicked() {
                        for (entity, _) in q_annotation.iter() {
                            match window.annotations.entry(entity) {
                                std::collections::hash_map::Entry::Occupied(mut entry) => {
                                    *entry.get_mut() = false;
                                }
                                std::collections::hash_map::Entry::Vacant(entry) => {
                                    entry.insert(false);
                                }
                            }
                        }
                    }
                });

                egui::Grid::new("annotations_grid")
                    .num_columns(3)
                    .show(ui, |ui| {
                        for (index, (entity, annotation)) in q_annotation.iter().enumerate() {
                            ui.horizontal(|ui| {
                                match window.annotations.entry(entity) {
                                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                                        ui.checkbox(entry.get_mut(), annotation.description());
                                    }
                                    std::collections::hash_map::Entry::Vacant(entry) => {
                                        ui.checkbox(entry.insert(false), annotation.description());
                                    }
                                }

                                show_color(ui, annotation.egui_colour(), egui::Vec2::splat(16.0));
                            });

                            if index % 3 == 2 {
                                ui.end_row();
                            }
                        }
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Classify").clicked() {
                        ui_events.send(UiEvent::Data(DataEvent::IMCEvent(
                            IMCEvent::GeneratePixelAnnotation {
                                labels: window
                                    .annotations
                                    .iter()
                                    .filter(|(_, included)| **included)
                                    .map(|(entity, _)| *entity)
                                    .collect(),
                                target: match window.target {
                                    ClassificationTarget::FieldOfView => {
                                        imc::PixelAnnotationTarget::Region(fov)
                                    }
                                    ClassificationTarget::WholeImage => {
                                        imc::PixelAnnotationTarget::Acquisitions(
                                            window
                                                .acquisitions
                                                .iter()
                                                .filter(|(_, included)| **included)
                                                .map(|(id, _)| id.clone())
                                                .collect(),
                                        )
                                    }
                                },
                                channels: channels
                                    .iter()
                                    .filter(|channel| {
                                        *window.channels.get(channel.label()).unwrap()
                                    })
                                    .map(|channel| {
                                        ChannelIdentifier::Label(channel.label().to_string())
                                    })
                                    .collect(),
                                output: ClassifierOutput::Window,
                            },
                        )));
                    }
                })
            });
    }
}
