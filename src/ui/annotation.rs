use bevy::prelude::*;
use bevy_egui::egui::{Color32, Ui};
// use egui::{Color32, Ui};
use rand::Rng;

use crate::{
    annotation::{Annotation, AnnotationEvent, Tool},
    camera::CameraCommand,
};

use super::{Editing, UiEvent, UiIcon, UiState};

pub(super) fn create_annotation_ui(world: &mut World, ui: &mut Ui) {
    let mut ui_events = Vec::new();

    world.resource_scope(|world, mut ui_state: Mut<UiState>| {
        bevy_egui::egui::Grid::new("annotation_grid")
            .num_columns(3)
            //.spacing([10.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                // Make sure annotations are always in the same order (sorting by entity)
                let mut q_annotations = world.query::<(Entity, &Annotation, &Visibility)>();

                let mut annotations = q_annotations
                    .iter(world)
                    .map(|(entity, annotation, visibility)| (entity, annotation, visibility)) // Copy out of the world
                    .collect::<Vec<_>>();
                annotations.sort_by(|a, b| a.0.cmp(&b.0));

                for (pencil_entity, annotation, visibility) in annotations {
                    let editing = world.get::<Editing>(pencil_entity).is_some();

                    if editing {
                        let mut annotation_name = annotation.description.to_string();

                        if ui.text_edit_singleline(&mut annotation_name).changed() {
                            ui_events.push(UiEvent::Annotation(AnnotationEvent::SetDescription {
                                entity: pencil_entity,
                                description: annotation_name,
                            }));
                        }
                    } else {
                        ui.label(annotation.description.to_string());
                    }
                    let mut colour = annotation.colour().egui();

                    if ui.color_edit_button_srgba(&mut colour).changed() {
                        ui_events.push(UiEvent::Annotation(AnnotationEvent::SetColour {
                            entity: pencil_entity,
                            colour: colour.into(),
                        }));
                    }

                    ui.horizontal(|ui| {
                        if editing {
                            let button = bevy_egui::egui::ImageButton::new(
                                ui_state.icon(UiIcon::EditOff),
                                bevy_egui::egui::Vec2::splat(ui_state.icon_size),
                            );

                            if ui
                                .add(button)
                                .on_hover_text("Editing annotation. Click to finish editing.")
                                .clicked()
                            {
                                ui_events.push(UiEvent::Annotation(AnnotationEvent::StopEdit));
                                ui_events.push(UiEvent::Camera(CameraCommand::EnableDragging));
                            }

                            if let Some(active_tool) = annotation.active_tool() {
                                match active_tool {
                                    Tool::Pencil { radius } => {
                                        let mut radius = radius;

                                        ui.style_mut().spacing.slider_width = 50.0;

                                        let radius_response = ui.add(
                                            bevy_egui::egui::Slider::new(&mut radius, 0.0..=200.0)
                                                .smart_aim(false)
                                                .orientation(
                                                    bevy_egui::egui::SliderOrientation::Horizontal,
                                                )
                                                .text("Radius"),
                                        );

                                        if radius_response.changed() {
                                            ui_events.push(UiEvent::Annotation(
                                                AnnotationEvent::SetActiveTool {
                                                    entity: pencil_entity,
                                                    active_tool: Some(Tool::Pencil { radius }),
                                                },
                                            ));
                                        }
                                    }
                                    Tool::Rubber { radius } => todo!(),
                                    Tool::Polygon {} => todo!(),
                                }
                            }
                        } else {
                            let button = bevy_egui::egui::ImageButton::new(
                                ui_state.icon(UiIcon::Edit),
                                bevy_egui::egui::Vec2::splat(ui_state.icon_size),
                            );

                            if ui
                                .add(button)
                                .on_hover_text("Click to enable editing annotation.")
                                .clicked()
                            {
                                ui_events.push(UiEvent::Annotation(AnnotationEvent::Show(
                                    pencil_entity,
                                )));
                                ui_events.push(UiEvent::Annotation(AnnotationEvent::Edit(
                                    pencil_entity,
                                )));
                                ui_events.push(UiEvent::Camera(CameraCommand::DisableDragging));
                            }

                            match visibility.is_visible {
                                true => {
                                    let visibility_button = bevy_egui::egui::ImageButton::new(
                                        ui_state.icon(UiIcon::Visible),
                                        bevy_egui::egui::Vec2::splat(ui_state.icon_size),
                                    );

                                    if ui
                                        .add(visibility_button)
                                        .on_hover_text("Showing annotation. Click to hide.")
                                        .clicked()
                                    {
                                        ui_events.push(UiEvent::Annotation(AnnotationEvent::Hide(
                                            pencil_entity,
                                        )))
                                    }
                                }
                                false => {
                                    let visibility_button = bevy_egui::egui::ImageButton::new(
                                        ui_state.icon(UiIcon::NotVisible),
                                        bevy_egui::egui::Vec2::splat(ui_state.icon_size),
                                    );

                                    if ui
                                        .add(visibility_button)
                                        .on_hover_text("Hiding annotation. Click to show.")
                                        .clicked()
                                    {
                                        ui_events.push(UiEvent::Annotation(AnnotationEvent::Show(
                                            pencil_entity,
                                        )))
                                    }
                                }
                            }

                            let button = bevy_egui::egui::ImageButton::new(
                                ui_state.icon(UiIcon::Remove),
                                bevy_egui::egui::Vec2::splat(ui_state.icon_size),
                            );

                            if ui
                                .add(button)
                                .on_hover_text(format!(
                                    "Remove {} annotation.",
                                    annotation.description
                                ))
                                .clicked()
                            {
                                ui_events.push(UiEvent::Annotation(AnnotationEvent::Remove(
                                    pencil_entity,
                                )));
                            }
                        }
                    });

                    // if let Some(children) = children {
                    //     if ui.button("Print").clicked() {
                    //         for line_entity in children.iter() {
                    //             if let Ok((_, line)) = q_pencil_line.get(*line_entity) {
                    //                 println!("Pixels {:?}", line.pixels);
                    //                 println!("Polygon {:?}", line.polygon);
                    //             }
                    //         }
                    //     }
                    // }

                    ui.end_row();
                }

                let button = bevy_egui::egui::ImageButton::new(
                    ui_state.icon(UiIcon::Add),
                    bevy_egui::egui::Vec2::splat(ui_state.icon_size),
                );

                // Add in extra row for adding a new annotation
                let annotation_name = ui_state.get_mut_string_with_default("annotation_name", "");
                if ui.text_edit_singleline(annotation_name).changed() {
                    // Nothing to do here
                };

                // let current_default_colour = ui_state.get_colour_with_default("annotation_default", Color32::)

                let annotation_colour =
                    ui_state.get_mut_colour_with_default("annotation_colour", Color32::YELLOW);
                if ui.color_edit_button_srgba(annotation_colour).changed() {
                    //ui_state.set_colour("annotation_colour", annotation_colour.clo)
                }

                if ui
                    .add(button)
                    .on_hover_text("Create annotation with the specified name.")
                    .clicked()
                {
                    ui_events.push(UiEvent::Annotation(AnnotationEvent::Add {
                        name: ui_state
                            .get_string("annotation_name")
                            .expect("We have already added the string with name 'annotation_name'")
                            .to_string(),
                        colour: ui_state.get_colour("annotation_colour").expect(
                            "We have already added the colour with identifier 'annotation_colour'",
                        ).into(),
                    }));
                }

                ui.end_row();
            });
        // });
    });

    for event in ui_events {
        world.send_event(event);
    }
}

// This system resets the annotation UI if an annotation has been added
pub(super) fn handle_add_annotation_event(
    mut ev_annotation: EventReader<AnnotationEvent>,
    mut ui_state: ResMut<UiState>,
) {
    for event in ev_annotation.iter() {
        // If we are adding an annotation, then we should reset the names
        if let AnnotationEvent::Add { name: _, colour: _ } = event {
            let mut rng = rand::thread_rng();

            ui_state.set_string("annotation_name", "".to_string());
            ui_state.set_colour(
                "annotation_colour",
                Color32::from_rgb(rng.gen(), rng.gen(), rng.gen()),
            );
        }
    }
}
