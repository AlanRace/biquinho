use std::collections::HashMap;

use bevy::{ecs::query::WorldQuery, prelude::*};

use bevy_egui::{
    egui::{
        self,
        plot::{Bar, BarChart, Plot},
        Color32, Label, ScrollArea, Slider, Ui,
    },
    EguiContext, EguiPlugin, EguiSettings,
};
use imc_rs::ChannelIdentifier;

use crate::{
    annotation::{AnnotationEvent, AnnotationPlugin},
    camera::{
        CameraEvent, CameraPlugin, CameraSetup, Draggable, FieldOfView, MousePosition, PanCamera,
        Selectable,
    },
    data::{CellSegmentation, DataEvent},
    image_plugin::{ImageControl, ImageEvent, Opacity},
    imc::{Acquisition, GenerateChannelImage, HistogramScale, IMCDataset, IMCEvent, LoadIMC},
    Message,
};

use self::annotation::{create_annotation_ui, handle_add_annotation_event};

mod annotation;
mod classification;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<UiEvent>()
        .add_plugin(EguiPlugin)
            .add_plugin(CameraPlugin { camera_setup: CameraSetup {
                x: 1, y: 1, margin: 10, names: vec![]
                // vec!["10^6 WT +50mpk".to_string(),  "10^6 VS".to_string(), "10^6 VS+100mpk".to_string(),
                // "10^6 WT +20mpk".to_string(),  "10^6 WT".to_string(), "Control".to_string()]
            }})
            .add_plugin(AnnotationPlugin)
            .init_resource::<UiState>() // This has to come after adding DefaultPlugins, or we won't have the AssetServer
            .insert_resource(UiSpace::default())
            // .add_event::<HideEvent>()
            .add_startup_system(configure_visuals)
            // .add_system(update_ui_scale_factor)
            .add_system(message_notification)
            .add_system(ui_right_panel_exclusive.label(UiLabel::Display))
            .add_system(imc_load_notification.after(UiLabel::Display))
            .add_system(ui_top_panel.label("top_panel").after(UiLabel::Display))
            .add_system(ui_bottom_panel.after("top_panel"))
            .add_system(
                handle_ui_events
                    .label(UiLabel::HandleUiEvent)
                    .after(UiLabel::Display),
            )
            .add_system(handle_add_annotation_event)
            .add_event::<UiEvent>()
            // .add_system(hide_children)
            // .add_system(handle_hide_event)
//            .add_plugin(ClassificationUiPlugin)
            ;
    }
}

fn configure_visuals(mut egui_ctx: ResMut<EguiContext>) {
    egui_ctx.ctx_mut().set_visuals(egui::Visuals {
        window_rounding: 0.0.into(),
        ..Default::default()
    });
}

fn update_ui_scale_factor(mut egui_settings: ResMut<EguiSettings>, windows: Res<Windows>) {
    if let Some(window) = windows.get_primary() {
        egui_settings.scale_factor = 1.0 / window.scale_factor();
    }
}

fn message_notification(
    mut commands: Commands,
    mut egui_ctx: ResMut<EguiContext>,
    q_errors: Query<(Entity, &Message)>,
) {
    for (entity, error) in q_errors.iter() {
        egui::Window::new(format!("{:?}", error.severity))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(egui_ctx.ctx_mut(), |ui| {
                ui.label(&error.message);

                if ui.button("Ok").clicked() {
                    commands.entity(entity).despawn();
                }
            });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemLabel)]
pub enum UiLabel {
    Display,
    HandleUiEvent,
}

pub enum UiEvent {
    Camera(CameraEvent),
    Annotation(AnnotationEvent),
    Image(ImageEvent),
    Data(DataEvent),
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum UiIcon {
    Visible,
    NotVisible,
    Add,
    Remove,
    Edit,
    EditOff,
    FolderOpen,
    Microscope,
}

impl UiIcon {
    fn path(&self) -> &str {
        match self {
            UiIcon::Visible => "icons/visibility_16px.png",
            UiIcon::NotVisible => "icons/visibility_off_16px.png",
            UiIcon::Add => "icons/add_16px.png",
            UiIcon::Remove => "icons/remove_16px.png",
            UiIcon::Edit => "icons/edit_16px.png",
            UiIcon::EditOff => "icons/edit_off_16px.png",
            UiIcon::FolderOpen => "icons/folder_open_16px.png",
            UiIcon::Microscope => "icons/biotech_64px.png",
        }
    }
}

#[derive(Debug, Resource)]
pub struct UiSpace {
    pub ui_space: UiRect,
}

impl UiSpace {
    pub fn right(&self) -> f32 {
        match self.ui_space.right {
            Val::Undefined => todo!(),
            Val::Auto => todo!(),
            Val::Px(pixel) => pixel,
            Val::Percent(_) => todo!(),
        }
    }

    pub fn top(&self) -> f32 {
        match self.ui_space.top {
            Val::Undefined => todo!(),
            Val::Auto => todo!(),
            Val::Px(pixel) => pixel,
            Val::Percent(_) => todo!(),
        }
    }

    pub fn left(&self) -> f32 {
        match self.ui_space.left {
            Val::Undefined => todo!(),
            Val::Auto => todo!(),
            Val::Px(pixel) => pixel,
            Val::Percent(_) => todo!(),
        }
    }

    pub fn bottom(&self) -> f32 {
        match self.ui_space.bottom {
            Val::Undefined => todo!(),
            Val::Auto => todo!(),
            Val::Px(pixel) => pixel,
            Val::Percent(_) => todo!(),
        }
    }
}

impl Default for UiSpace {
    fn default() -> Self {
        Self {
            ui_space: UiRect {
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
            },
        }
    }
}

#[derive(Resource)]
pub struct UiState {
    bevy_icons: HashMap<UiIcon, Handle<Image>>,
    egui_icons: HashMap<UiIcon, egui::TextureId>,

    string_values: HashMap<String, String>,
    colour_values: HashMap<String, Color32>,

    icon_size: f32,

    last_mz_ppm: (f64, f64),

    combo_box_selection: HashMap<Entity, usize>,
    selected_channel: usize,
    // annotation: AnnotationUiState,
}

impl UiState {
    fn load_icon(&mut self, asset_server: &mut Mut<AssetServer>, icon: UiIcon) {
        self.bevy_icons
            .insert(icon.clone(), asset_server.load(icon.path()));
    }

    fn icon(&self, icon: UiIcon) -> egui::TextureId {
        *self.egui_icons.get(&icon).unwrap()
        // match self.egui_icons.entry(icon.clone()) {
        //     Entry::Occupied(entry) => *entry.get(),
        //     Entry::Vacant(entry) => {
        //         let texture_id =
        //             egui_ctx.add_image(self.bevy_icons.get(&icon).unwrap().clone_weak());

        //         entry.insert(texture_id);

        //         texture_id
        //     }
        // }
    }

    fn set_string(&mut self, identifier: &str, value: String) {
        self.string_values.insert(identifier.to_string(), value);
    }

    fn get_string(&self, identifier: &str) -> Option<&String> {
        self.string_values.get(identifier)
    }

    fn get_mut_string_with_default(&mut self, identifier: &str, default: &str) -> &mut String {
        if !self.string_values.contains_key(identifier) {
            self.string_values
                .insert(identifier.to_string(), default.to_string());
        }

        self.string_values.get_mut(identifier).unwrap()
    }

    fn set_colour(&mut self, identifier: &str, value: Color32) {
        self.colour_values.insert(identifier.to_string(), value);
    }

    fn get_colour(&self, identifier: &str) -> Option<&Color32> {
        self.colour_values.get(identifier)
    }

    fn get_colour_with_default(&mut self, identifier: &str, default: Color32) -> &Color32 {
        if !self.colour_values.contains_key(identifier) {
            self.colour_values.insert(identifier.to_string(), default);
        }

        self.colour_values.get(identifier).unwrap()
    }

    fn get_mut_colour_with_default(&mut self, identifier: &str, default: Color32) -> &mut Color32 {
        if !self.colour_values.contains_key(identifier) {
            self.colour_values.insert(identifier.to_string(), default);
        }

        self.colour_values.get_mut(identifier).unwrap()
    }
}

impl FromWorld for UiState {
    fn from_world(world: &mut World) -> Self {
        let mut asset_server = world.get_resource_mut::<AssetServer>().unwrap();

        let mut ui_state = Self {
            bevy_icons: HashMap::new(),
            egui_icons: HashMap::new(),

            string_values: HashMap::new(),
            colour_values: HashMap::new(),

            icon_size: 16.0,

            last_mz_ppm: (0.0, 0.0),
            combo_box_selection: HashMap::new(),
            selected_channel: 0,
            // annotation: AnnotationUiState::default(),
        };

        ui_state.load_icon(&mut asset_server, UiIcon::Visible);
        ui_state.load_icon(&mut asset_server, UiIcon::NotVisible);
        ui_state.load_icon(&mut asset_server, UiIcon::Add);
        ui_state.load_icon(&mut asset_server, UiIcon::Remove);
        ui_state.load_icon(&mut asset_server, UiIcon::Edit);
        ui_state.load_icon(&mut asset_server, UiIcon::EditOff);
        ui_state.load_icon(&mut asset_server, UiIcon::FolderOpen);
        ui_state.load_icon(&mut asset_server, UiIcon::Microscope);

        let mut egui_ctx = world.get_resource_mut::<EguiContext>().unwrap();
        for (icon, handle) in &ui_state.bevy_icons {
            ui_state
                .egui_icons
                .insert(icon.clone(), egui_ctx.add_image(handle.clone_weak()));
        }

        ui_state
    }
}

fn handle_ui_events(
    mut ui_events: EventReader<UiEvent>,
    mut ui_state: ResMut<UiState>,
    mut camera_events: EventWriter<CameraEvent>,
    mut annotation_events: EventWriter<AnnotationEvent>,
    mut image_events: EventWriter<ImageEvent>,
    mut data_events: EventWriter<DataEvent>,
) {
    for event in ui_events.iter() {
        match event {
            UiEvent::Camera(event) => {
                camera_events.send(event.clone());
            }
            UiEvent::Annotation(event) => {
                annotation_events.send(event.clone());
            }
            UiEvent::Image(event) => {
                image_events.send(*event);
            }
            UiEvent::Data(event) => {
                data_events.send(event.clone());
            }
        }
    }
}

// struct AnnotationUiState {
//     name: String,
//     colour: Color32,
// }

// impl Default for AnnotationUiState {
//     fn default() -> Self {
//         let mut rng = rand::thread_rng();

//         Self {
//             name: Default::default(),
//             colour: Color32::from_rgb(rng.gen(), rng.gen(), rng.gen()),
//         }
//     }
// }

#[derive(Component)]
struct Hideable;

// #[derive(Clone, Copy)]
// pub enum HideEvent {
//     Show(Entity),
//     Hide(Entity),
// }

// Handle events that are fired by the interface
// fn handle_hide_event(
//     mut ev_annotation: EventReader<HideEvent>,
//     mut q_hideable: Query<&mut Visibility, With<Hideable>>,
// ) {
//     for event in ev_annotation.iter() {
//         match event {
//             HideEvent::Hide(entity) => {
//                 if let Ok(mut visibility) = q_hideable.get_mut(*entity) {
//                     visibility.is_visible = false;
//                 }
//             }
//             HideEvent::Show(entity) => {
//                 if let Ok(mut visibility) = q_hideable.get_mut(*entity) {
//                     visibility.is_visible = true;
//                 }
//             }
//         }
//     }
// }

// // If we hide a parent annotation, then ideally we should hide all children, so fire more events
// // for each child if a change in the visibility is detected
// fn hide_children(
//     mut ev_annotation: EventWriter<HideEvent>,
//     q_annotations: Query<(&Children, &Visibility), (With<Hideable>, Changed<Visibility>)>,
// ) {
//     for (children, visibility) in q_annotations.iter() {
//         if !visibility.is_visible {
//             for &child in children.iter() {
//                 ev_annotation.send(HideEvent::Hide(child))
//             }
//         }
//     }
// }

#[derive(Component)]
pub struct PrimaryUiEntry {
    pub description: String,
}

#[derive(Component)]
pub struct UiEntry {
    pub description: String,
}

// This system cannot run in parallel with any other system
// This is to avoid needing a large function with many arguments
// Pollibly better solution: https://github.com/bevyengine/bevy/discussions/5522
fn ui_right_panel_exclusive(world: &mut World) {
    world.resource_scope(|world, mut egui_ctx: Mut<EguiContext>| {
        egui::SidePanel::right("side_panel")
            .default_width(300.0)
            .show(egui_ctx.ctx_mut(), |ui| {
                let side_panel_size = ui.available_size();

                // Update the side panel size
                world.resource_scope(|world, mut ui_space: Mut<UiSpace>| {
                    let estimated_panel_size = side_panel_size.x + 18.0;

                    // Update the viewport stored if it is different.
                    // Check is included here to avoid is_changed() being activated each frame
                    if ui_space.ui_space.right != Val::Px(estimated_panel_size) {
                        ui_space.ui_space.right = Val::Px(estimated_panel_size);
                    }
                });

                ui_camera_panel(world, ui);
                ui.separator();

                ui_data_panel(world, ui, side_panel_size.y * 0.4);
                ui.separator();

                ui.collapsing("Annotations", |ui| {
                    ui.set_max_height(side_panel_size.y * 0.25);

                    ScrollArea::vertical().auto_shrink([true; 2]).show_viewport(
                        ui,
                        |ui, viewport| {
                            // if ui.button("Process annotations!").clicked() {
                            //     let annotations: Vec<Entity> =
                            //         q_annotations.iter().map(|(entity, _, _)| entity).collect();
                            //     let (imc, _) = q_imc.get_single().unwrap();

                            //     let acquisitions = vec![imc.acquisitions[0]];

                            // }

                            create_annotation_ui(world, ui);
                        },
                    );
                });

                ui.separator();

                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show_viewport(ui, |ui, viewport| {
                        ui_imc_panel(world, ui);

                        #[cfg(feature = "msi")]
                        ui_msi_panel(world, ui);
                    });
            });
    });
}

fn ui_imc_panel(world: &mut World, ui: &mut Ui) {
    let mut q_imc = world.query::<(Entity, &IMCDataset, &Children)>();
    // let commands = world.co

    let mut ui_events = Vec::new();
    let mut generation_events = Vec::new();

    world.resource_scope(|world: &mut World, mut ui_state: Mut<UiState>| {
        for (entity, imc, children) in q_imc.iter(world) {
            // ui.collapsing(heading, add_contents);

            let id = ui.make_persistent_id(format!("header_for_{:?}", entity));

            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    ui.image(
                        ui_state.icon(UiIcon::Microscope),
                        egui::Vec2::splat(ui_state.icon_size),
                    );

                    ui.label(imc.name());
                    // ui.heading(format!("IMC {}", imc.name()));

                    // ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    //     let width = side_panel_size.x - 80.0;
                    //     let char_width = 6.0;
                    //     let num_chars =
                    //         ((width / char_width).floor() as usize).min(description.len());

                    //     ui.add_sized(
                    //         [num_chars as f32 * char_width, 10.0],
                    //         egui::Label::new(&description[..num_chars]),
                    //     );

                    //     ui.style_mut().visuals.override_text_color = Some(Color32::RED);
                    //     let close_response = ui.button("X").on_hover_text(
                    //         "Close the data, removing it and any children from the interface.",
                    //     );

                    //     if close_response.clicked() {
                    //         // world.send_event(UiEvent::Data(DataEvent::CloseData(entity)));
                    //         events.push(UiEvent::Data(DataEvent::CloseData(entity)));
                    //     }
                    // });
                })
                .body(|ui| {
                    // IMCGrid::new().ui(ui, imc, children, &mut ui_events);

                    // General IMC contols
                    egui::Grid::new(format!("{}_{:?}", "imc_controls", entity))
                        .num_columns(2)
                        .spacing([40.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Background opacity");
                            let mut alpha = imc.background_alpha();

                            let opacity = ui.add(
                                Slider::new(&mut alpha, 0.0..=1.0)
                                    .step_by(0.01)
                                    .clamp_to_range(true)
                                    .orientation(egui::SliderOrientation::Horizontal),
                            );
                            if opacity.changed() {
                                ui_events.push(UiEvent::Data(DataEvent::IMCEvent(
                                    IMCEvent::SetBackgroundOpacity {
                                        entity,
                                        opacity: alpha,
                                    },
                                )));
                            }

                            ui.end_row();

                            let mut histogram_scale = *imc.histogram_scale();
                            ui.label("Histogram scaling");

                            egui::ComboBox::from_id_source(format!(
                                "{}_{:?}",
                                "histogram_scale", entity
                            ))
                            .selected_text(format!("{:?}", histogram_scale))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut histogram_scale,
                                    HistogramScale::None,
                                    "None",
                                );
                                ui.selectable_value(
                                    &mut histogram_scale,
                                    HistogramScale::Log10,
                                    "log10",
                                );
                                ui.selectable_value(&mut histogram_scale, HistogramScale::Ln, "ln");
                            });

                            if histogram_scale != *imc.histogram_scale() {
                                ui_events.push(UiEvent::Data(DataEvent::IMCEvent(
                                    IMCEvent::SetHistogramScale {
                                        entity,
                                        scale: histogram_scale,
                                    },
                                )));
                            }
                        });

                    for child in children.iter() {
                        let control = world.get::<ImageControl>(*child);

                        if let Some(control) = control {
                            let control_entity = *child;

                            let selection = ui_state
                                .combo_box_selection
                                .entry(control_entity)
                                .or_insert(0);

                            let channels = imc.channels();

                            egui::Grid::new(format!("{}_{:?}", "marker_grid", control_entity))
                                .num_columns(2)
                                .spacing([40.0, 4.0])
                                .show(ui, |ui| {
                                    ui.add(Label::new(&control.description));

                                    let selected_text = if *selection == 0 {
                                        "None"
                                    } else if channels[*selection - 1].label().trim().is_empty() {
                                        channels[*selection - 1].name()
                                    } else {
                                        channels[*selection - 1].label()
                                    };

                                    egui::ComboBox::from_id_source(control_entity)
                                        .width(100.0)
                                        .selected_text(selected_text)
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_value(selection, 0, "None").clicked() {
                                                generation_events.push((
                                                    control_entity,
                                                    GenerateChannelImage { identifier: None },
                                                ));
                                            }

                                            for (index, channel) in channels.iter().enumerate() {
                                                let name = if channel.label().trim().is_empty() {
                                                    channel.name()
                                                } else {
                                                    channel.label()
                                                };

                                                if ui
                                                    .selectable_value(selection, index + 1, name)
                                                    .clicked()
                                                {
                                                    // TODO: Send out event that we should generate ion image
                                                    println!("Selected {}", channel.name());
                                                    // world.entity_mut(control_entity).insert(
                                                    //     GenerateChannelImage {
                                                    //         identifier: ChannelIdentifier::Name(
                                                    //             channel.name().into(),
                                                    //         ),
                                                    //     },
                                                    // );
                                                    generation_events.push((
                                                        control_entity,
                                                        GenerateChannelImage {
                                                            identifier: Some(
                                                                ChannelIdentifier::Name(
                                                                    channel.name().into(),
                                                                ),
                                                            ),
                                                        },
                                                    ));
                                                }
                                            }
                                        });
                                });

                            let intensity_range = control.intensity_range;

                            if !control.histogram.is_empty() {
                                let num_bins = control.histogram.len();

                                let bin_size =
                                    (intensity_range.1 - intensity_range.0) / (num_bins - 1) as f32;

                                let chart = BarChart::new(
                                    (0..num_bins)
                                        .map(|x| {
                                            Bar::new(
                                                (x as f32 * bin_size + control.intensity_range.0)
                                                    as f64,
                                                match imc.histogram_scale() {
                                                    HistogramScale::None => {
                                                        control.histogram[x] as f64
                                                    }
                                                    HistogramScale::Log10 => {
                                                        (control.histogram[x] as f64 + 1.0).log10()
                                                    }
                                                    HistogramScale::Ln => {
                                                        (control.histogram[x] as f64 + 1.0).ln()
                                                    }
                                                },
                                            )
                                            .width(bin_size as f64)
                                        })
                                        .collect(),
                                )
                                .color(Color32::LIGHT_BLUE);

                                Plot::new(format!("{}_{:?}", "histogram", control_entity))
                                    .height(75.0)
                                    .show(ui, |plot_ui| plot_ui.bar_chart(chart));
                            }

                            let mut min_value = control.colour_domain.0;

                            let min_value_response = ui.add(
                                Slider::new(&mut min_value, intensity_range.0..=intensity_range.1)
                                    .clamp_to_range(true)
                                    .smart_aim(false)
                                    .orientation(egui::SliderOrientation::Horizontal)
                                    .text("Min"),
                            );

                            let mut max_value = control.colour_domain.1;

                            let max_value_response = ui.add(
                                Slider::new(&mut max_value, intensity_range.0..=intensity_range.1)
                                    .clamp_to_range(true)
                                    .smart_aim(false)
                                    .orientation(egui::SliderOrientation::Horizontal)
                                    .text("Max"),
                            );

                            if min_value_response.changed() || max_value_response.changed() {
                                if min_value > max_value {
                                    min_value = max_value;
                                }

                                // Avoid double sending the event due to delay in event propagation
                                ui_events.push(UiEvent::Image(ImageEvent::SetColourDomain(
                                    control_entity,
                                    (min_value, max_value),
                                )));
                            }

                            ui.separator();
                        }
                    }
                });
        }
    });

    for (entity, generation) in generation_events {
        world.entity_mut(entity).insert(generation);
    }

    for event in ui_events {
        world.send_event(event);
    }
}

#[cfg(feature = "msi")]
fn ui_msi_panel(world: &mut World, ui: &mut Ui) {
    let mut generate_events = Vec::new();

    world.resource_scope(|mut world, mut ui_state: Mut<UiState>| {
        let mut q_msi = world.query::<(Entity, &MSIDataset, Option<&Children>)>();
        for (entity, msi, children) in q_msi.iter(world) {
            //msi.ui(ui);
            ui.label(msi.name());

            let (mut mz, mut ppm): (f64, f64) = ui_state.last_mz_ppm;
            let (min, max) = msi.mz_range();

            let mz_response = ui.add(
                Slider::new(&mut mz, min..=max)
                    .min_decimals(5)
                    .max_decimals(5)
                    .clamp_to_range(true)
                    .smart_aim(false)
                    .orientation(egui::SliderOrientation::Horizontal)
                    .text("m/z"),
            );

            let ppm_response = ui.add(
                Slider::new(&mut ppm, 0.0..=1000.0)
                    .clamp_to_range(true)
                    .smart_aim(false)
                    .orientation(egui::SliderOrientation::Horizontal)
                    .text("ppm"),
            );

            if mz_response.changed() || ppm_response.changed() {
                ui_state.last_mz_ppm = (mz, ppm);

                generate_events.push((entity, GenerateIonImage { mz, ppm }));
            }

            // let opacity_response = ui.add(
            //     Slider::new(&mut ppm, 0.0..=100.0)
            //         .clamp_to_range(true)
            //         .smart_aim(true)
            //         .orientation(egui::SliderOrientation::Horizontal)
            //         .text("Opacity"),
            // );
            // if opacity_response.changed() {
            //     // Avoid double sending the event due to delay in event propagation
            //     ui_events.send(UiEvent::ImageEvent(ImageEvent::ChangeOpacity(
            //         control_entity,
            //         opacity_response,
            //     )));
            // }

            // Annotations
            // if let Some(children) = children {
            //     AnnotationGrid::new().ui(
            //         ui,
            //         children,
            //         &mut ui_events,
            //         &mut q_annotations,
            //     );
            // }

            // let mut plot = spectrum::SpectrumPlot::new();

            // let mut q_marker = world.query::<&mut SpectrumMarker>();

            // for marker in q_marker.iter_mut(world) {
            //     if let Some(spectrum) = &marker.spectrum {
            //         let mut spectrum: spectrum::Spectrum = spectrum.into();

            //         spectrum.set_colour(marker.colour);
            //         plot.add_spectrum(spectrum);
            //     }
            // }

            // ui.add(plot);

            // // Spectrum markers
            // egui::Grid::new("marker_grid")
            //     .num_columns(2)
            //     .spacing([40.0, 4.0])
            //     .striped(true)
            //     .show(ui, |ui| {
            //         for mut marker in q_marker.iter_mut(world) {
            //             ui.label(format!("Marker {:?}", marker.spectrum_id));
            //             ui.color_edit_button_srgba(&mut marker.colour);
            //             ui.end_row();
            //         }
            //     });
        }
    });

    for (entity, generate) in generate_events {
        world.entity_mut(entity).insert(generate);
    }
}

fn ui_camera_panel(world: &mut World, ui: &mut Ui) {
    let mut camera_events = Vec::new();

    world.resource_scope(|world, mut ui_state: Mut<UiState>| {
        ui.collapsing("Camera", |ui| {
            ScrollArea::vertical()
                .auto_shrink([true; 2])
                .show_viewport(ui, |ui, viewport| {
                    let camera_setup = world.resource::<CameraSetup>();

                    ui.horizontal(|ui| {
                        ui.label("Grid size");
                        let x = ui_state.get_mut_string_with_default(
                            "camera_x_value",
                            &format!("{}", camera_setup.x),
                        );
                        let x_edit = egui::TextEdit::singleline(x).desired_width(20.0);

                        if x_edit.show(ui).response.changed() {
                            if let Ok(x) = x.parse::<u32>() {
                                camera_events.push(CameraEvent::SetGrid((x, camera_setup.y)));
                            }
                        }
                        ui.label(" x ");

                        let y = ui_state.get_mut_string_with_default(
                            "camera_y_value",
                            &format!("{}", camera_setup.y),
                        );
                        let y_edit = egui::TextEdit::singleline(y).desired_width(20.0);
                        if y_edit.show(ui).response.changed() {
                            if let Ok(y) = y.parse::<u32>() {
                                camera_events.push(CameraEvent::SetGrid((camera_setup.x, y)));
                            }
                        }
                    });

                    // Determine the z-position of the camera(s)
                    let mut q_transform = world.query::<(&Transform, &PanCamera)>();

                    if let Some((transform, _)) = q_transform.iter(world).next() {
                        ui.horizontal(|ui| {
                            ui.label("Zoom");

                            let mut scale_value = transform.scale.x;

                            let response = ui.add(
                                Slider::new(&mut scale_value, 0.01..=1000.0)
                                    // .clamp_to_range(true)
                                    .logarithmic(true)
                                    .smart_aim(false)
                                    .orientation(egui::SliderOrientation::Horizontal)
                                    .text(""),
                            );

                            if response.changed() {
                                camera_events.push(CameraEvent::Zoom(scale_value));
                            }
                        });
                    }

                    egui::Grid::new("camera_grid")
                        .num_columns(2)
                        .spacing([40.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            let mut q_imc =
                                world.query::<(&Acquisition, &UiEntry, &GlobalTransform)>();
                            let mut q_camera = world.query::<(Entity, &PanCamera)>();

                            let mut cameras = q_camera.iter(world).collect::<Vec<_>>();
                            cameras.sort_by(|(entity_a, camera_a), (entity_b, camera_b)| {
                                if camera_a.y == camera_b.y {
                                    camera_a.x.cmp(&camera_b.x)
                                } else {
                                    camera_a.y.cmp(&camera_b.y)
                                }
                            });

                            for (entity, camera) in cameras {
                                let text = world.get::<Text>(camera.camera_text).unwrap();

                                ui.label(format!("Camera ({}, {})", camera.x, camera.y));

                                let mut camera_name = text.sections[0].value.clone();
                                if ui.text_edit_singleline(&mut camera_name).changed() {
                                    camera_events.push(CameraEvent::SetName((entity, camera_name)));
                                }
                                // ui.color_edit_button_srgba(&mut camera.colour);
                                ui.end_row();

                                ui.label("Look at");

                                let mut selection = 0;

                                egui::ComboBox::from_id_source(entity)
                                    .selected_text("None")
                                    .show_ui(ui, |ui| {
                                        for (index, (acquisition, ui_entry, transform)) in
                                            q_imc.iter(world).enumerate()
                                        {
                                            if ui
                                                .selectable_value(
                                                    &mut selection,
                                                    index,
                                                    &ui_entry.description,
                                                )
                                                .clicked()
                                            {
                                                // TODO: Send out event that we should generate ion image
                                                println!("Selected {}", ui_entry.description);
                                                // generation_events.push((
                                                //     control_entity,
                                                //     GenerateChannelImage {
                                                //         identifier: ChannelIdentifier::Name(
                                                //             channel.name().into(),
                                                //         ),
                                                //     },
                                                // ));
                                                camera_events.push(CameraEvent::SetName((
                                                    entity,
                                                    ui_entry.description.clone(),
                                                )));
                                                camera_events.push(CameraEvent::LookAt((
                                                    entity,
                                                    transform.translation(),
                                                )));
                                            }
                                        }
                                    });

                                ui.end_row();
                            }
                        });
                });
        });
    });

    for event in camera_events {
        // println!("Sending camera event");
        world.send_event(event);
    }
}

fn ui_data_panel(world: &mut World, ui: &mut Ui, max_height: f32) {
    ui.collapsing("Data", |ui| {
        ui.set_max_height(max_height);

        ScrollArea::both()
            .auto_shrink([true; 2])
            .show_viewport(ui, |ui, viewport| {
                let side_panel_size = ui.available_size();

                let mut q_primary = world.query::<(Entity, &PrimaryUiEntry, &Children)>();

                let mut events = Vec::new();
                let mut data: Vec<(Entity, String)> = Vec::new();

                for (entity, primary_entry, children) in q_primary.iter(world) {
                    data.push((entity, primary_entry.description.clone()));
                }

                drop(q_primary);

                for (entity, description) in data {
                    let id = ui.make_persistent_id(format!("header_for_{:?}", entity));
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        id,
                        false,
                    )
                    .show_header(ui, |ui| {
                        // egui::Grid::new(format!("grid_for_{:?}", entity))
                        //     .num_columns(2)
                        //     //.spacing([10.0, 4.0])
                        //     .striped(true)
                        //     .show(ui, |ui| {

                        //     })
                        //     .response
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            let width = side_panel_size.x - 80.0;
                            let char_width = 6.0;
                            let num_chars =
                                ((width / char_width).floor() as usize).min(description.len());

                            ui.add_sized(
                                [num_chars as f32 * char_width, 10.0],
                                egui::Label::new(&description[..num_chars]),
                            );

                            ui.style_mut().visuals.override_text_color = Some(Color32::RED);
                            let close_response = ui.button("X").on_hover_text(
                                "Close the data, removing it and any children from the interface.",
                            );

                            if close_response.clicked() {
                                // world.send_event(UiEvent::Data(DataEvent::CloseData(entity)));
                                events.push(UiEvent::Data(DataEvent::CloseData(entity)));
                            }
                        });
                    })
                    .body(|ui| {
                        add_children_to_ui_world(entity, ui, world);
                    });
                    // egui::CollapsingHeader::new(format!("{:?}", primary_entry.description))
                    //     .show(ui, |ui| {
                    //         add_children_to_ui(
                    //             entity,
                    //             ui,
                    //             children,
                    //             &q_ui_entry,
                    //             &mut ui_events,
                    //         );
                    //     });
                }

                for event in events {
                    world.send_event(event);
                }
            });
    });
}

// Idea to avoid this large function: https://github.com/bevyengine/bevy/discussions/5522
// #[allow(clippy::too_many_arguments)]
// fn ui_right_panel(
//     mut commands: Commands,
//     mut egui_ctx: ResMut<EguiContext>,
//     q_primary: Query<(Entity, &PrimaryUiEntry, &Children)>,
//     q_ui_entry: Query<UiControllable>,
//     //q_pencil_annotation: Query<(Entity, &PencilAnnotation)>,
//     q_editing: Query<&Editing>,
//     q_imc: Query<(&IMCDataset, &Children)>,
//     q_msi: Query<(Entity, &MSIDataset, Option<&Children>)>,
//     mut ui_events: EventWriter<UiEvent>,
//     q_annotations: Query<(Entity, &Annotation, &Visibility)>,
//     //mut q_annotations: Query<(&mut Annotation, &Children, &mut DrawMode, &Visibility)>,
//     mut q_marker: Query<&mut SpectrumMarker>,
//     q_control: Query<(Entity, &ImageControl)>,
//     mut ui_state: ResMut<UiState>,
//     mut ui_space: ResMut<UiSpace>,
// ) {
//     //let camera_transform = q_camera.single();
//     //let window = windows.get_primary().unwrap();
//     //let window_size = Vec2::new(window.width() as f32, window.height() as f32);

//     //let pos_in_world = window
//     //    .cursor_position()
//     //    .map(|cur_mouse_pos| camera_to_world(cur_mouse_pos, window_size, camera_transform));

//     egui::SidePanel::right("side_panel")
//         .default_width(300.0)
//         .show(egui_ctx.ctx_mut(), |ui| {
//             let side_panel_size = ui.available_size();

//             let estimated_panel_size = side_panel_size.x + 18.0;

//             // Update the viewport stored if it is different.
//             // Check is included here to avoid is_changed() being activated each frame
//             if ui_space.ui_space.right != Val::Px(estimated_panel_size) {
//                 ui_space.ui_space.right = Val::Px(estimated_panel_size);
//             }

//             ui.collapsing("Camera", |ui| {
//                 ScrollArea::vertical()
//                     .auto_shrink([true; 2])
//                     .show_viewport(ui, |ui, viewport| {
//                         egui::Grid::new("camera_grid")
//                             .num_columns(2)
//                             .spacing([40.0, 4.0])
//                             .striped(true)
//                             .show(ui, |ui| {
//                                 for mut marker in q_marker.iter_mut() {
//                                     ui.label(format!("Marker {:?}", marker.spectrum_id));
//                                     ui.color_edit_button_srgba(&mut marker.colour);
//                                     ui.end_row();
//                                 }
//                             });
//                     });
//             });

//             ui.separator();

//             ui.collapsing("Data", |ui| {
//                 ui.set_max_height(side_panel_size.y * 0.25);

//                 ScrollArea::both()
//                     .auto_shrink([true; 2])
//                     .show_viewport(ui, |ui, viewport| {
//                         for (entity, primary_entry, children) in q_primary.iter() {
//                             //ui.heading(primary_entry.description.to_string());

//                             let id = ui.make_persistent_id(format!("header_for_{:?}", entity));
//                             egui::collapsing_header::CollapsingState::load_with_default_open(
//                                 ui.ctx(),
//                                 id,
//                                 false,
//                             )
//                             .show_header(ui, |ui| {
//                                 // egui::Grid::new(format!("grid_for_{:?}", entity))
//                                 //     .num_columns(2)
//                                 //     //.spacing([10.0, 4.0])
//                                 //     .striped(true)
//                                 //     .show(ui, |ui| {

//                                 //     })
//                                 //     .response
//                                 ui.with_layout(
//                                     egui::Layout::left_to_right(egui::Align::Min),
//                                     |ui| {
//                                         let width = side_panel_size.x - 80.0;
//                                         let char_width = 6.0;
//                                         let num_chars = ((width / char_width).floor() as usize)
//                                             .min(primary_entry.description.len());

//                                         ui.add_sized(
//                                             [num_chars as f32 * char_width, 10.0],
//                                             egui::Label::new(
//                                                 &primary_entry.description[..num_chars],
//                                             ),
//                                         );

//                                         ui.style_mut().visuals.override_text_color =
//                                             Some(Color32::RED);
//                                         let close_response = ui.button("X").on_hover_text(
//                                 "Close the data, removing it and any children from the interface.",
//                             );

//                                         if close_response.clicked() {
//                                             ui_events
//                                                 .send(UiEvent::Data(DataEvent::CloseData(entity)));
//                                         }
//                                     },
//                                 );
//                             })
//                             .body(|ui| {
//                                 add_children_to_ui(
//                                     entity,
//                                     ui,
//                                     children,
//                                     &q_ui_entry,
//                                     &mut ui_events,
//                                     &ui_state,
//                                 );
//                             });
//                             // egui::CollapsingHeader::new(format!("{:?}", primary_entry.description))
//                             //     .show(ui, |ui| {
//                             //         add_children_to_ui(
//                             //             entity,
//                             //             ui,
//                             //             children,
//                             //             &q_ui_entry,
//                             //             &mut ui_events,
//                             //         );
//                             //     });
//                         }
//                     });
//             });

//             ui.separator();

//             ui.collapsing("Annotations", |ui| {
//                 ui.set_max_height(side_panel_size.y * 0.25);

//                 ScrollArea::vertical()
//                     .auto_shrink([true; 2])
//                     .show_viewport(ui, |ui, viewport| {
//                         // if ui.button("Process annotations!").clicked() {
//                         //     let annotations: Vec<Entity> =
//                         //         q_annotations.iter().map(|(entity, _, _)| entity).collect();
//                         //     let (imc, _) = q_imc.get_single().unwrap();

//                         //     let acquisitions = vec![imc.acquisitions[0]];

//                         // }

//                         create_annotation_ui(
//                             ui,
//                             &mut ui_state,
//                             q_editing,
//                             q_annotations,
//                             &mut ui_events,
//                         );
//                     });
//             });

//             ui.separator();

//             ScrollArea::vertical()
//                 .auto_shrink([false; 2])
//                 .show_viewport(ui, |ui, viewport| {
//                     for (imc, children) in q_imc.iter() {
//                         ui.heading(format!("IMC {}", imc.name()));

//                         IMCGrid::new().ui(ui, imc, children);

//                         for child in children.iter() {
//                             if let Ok((control_entity, control)) = q_control.get(*child) {
//                                 let selection = ui_state
//                                     .combo_box_selection
//                                     .entry(control_entity)
//                                     .or_insert(0);

//                                 let channels = imc.channels();

//                                 egui::Grid::new(format!("{}_{:?}", "marker_grid", control_entity))
//                                     .num_columns(2)
//                                     .spacing([40.0, 4.0])
//                                     .show(ui, |ui| {
//                                         ui.add(Label::new("Channel"));
//                                         egui::ComboBox::from_id_source(control_entity)
//                                             .selected_text(channels[*selection].label().to_string())
//                                             .show_ui(ui, |ui| {
//                                                 for (index, channel) in channels.iter().enumerate()
//                                                 {
//                                                     if ui
//                                                         .selectable_value(
//                                                             selection,
//                                                             index,
//                                                             channel.label(),
//                                                         )
//                                                         .clicked()
//                                                     {
//                                                         // TODO: Send out event that we should generate ion image
//                                                         println!("Selected {}", channel.name());
//                                                         commands.entity(control_entity).insert(
//                                                             GenerateChannelImage {
//                                                                 identifier: ChannelIdentifier::Name(
//                                                                     channel.name().into(),
//                                                                 ),
//                                                             },
//                                                         );
//                                                     }
//                                                 }
//                                             });
//                                     });

//                                 let intensity_range = control.intensity_range;

//                                 if !control.histogram.is_empty() {
//                                     let num_bins = control.histogram.len();

//                                     let bin_size = (intensity_range.1 - intensity_range.0)
//                                         / (num_bins - 1) as f32;

//                                     let chart = BarChart::new(
//                                         (0..num_bins)
//                                             .map(|x| {
//                                                 Bar::new(
//                                                     (x as f32 * bin_size
//                                                         + control.intensity_range.0)
//                                                         as f64,
//                                                     (control.histogram[x] as f64 + 1.0).log10(),
//                                                 )
//                                                 .width(bin_size as f64)
//                                             })
//                                             .collect(),
//                                     )
//                                     .color(Color32::LIGHT_BLUE);

//                                     Plot::new(format!("{}_{:?}", "histogram", control_entity))
//                                         .height(75.0)
//                                         .show(ui, |plot_ui| plot_ui.bar_chart(chart));
//                                 }

//                                 let mut min_value = control.colour_domain.0;

//                                 let min_value_response = ui.add(
//                                     Slider::new(
//                                         &mut min_value,
//                                         intensity_range.0..=intensity_range.1,
//                                     )
//                                     .clamp_to_range(true)
//                                     .smart_aim(false)
//                                     .orientation(egui::SliderOrientation::Horizontal)
//                                     .text("Min"),
//                                 );

//                                 let mut max_value = control.colour_domain.1;

//                                 let max_value_response = ui.add(
//                                     Slider::new(
//                                         &mut max_value,
//                                         intensity_range.0..=intensity_range.1,
//                                     )
//                                     .clamp_to_range(true)
//                                     .smart_aim(false)
//                                     .orientation(egui::SliderOrientation::Horizontal)
//                                     .text("Max"),
//                                 );

//                                 if min_value_response.changed() || max_value_response.changed() {
//                                     // Avoid double sending the event due to delay in event propagation
//                                     ui_events.send(UiEvent::Image(ImageEvent::SetColourDomain(
//                                         control_entity,
//                                         (min_value, max_value),
//                                     )));
//                                 }
//                             }
//                         }
//                     }

//                     for (entity, msi, children) in q_msi.iter() {
//                         //msi.ui(ui);
//                         ui.label(msi.name());

//                         let (mut mz, mut ppm): (f64, f64) = ui_state.last_mz_ppm;
//                         let (min, max) = msi.mz_range();

//                         let mz_response = ui.add(
//                             Slider::new(&mut mz, min..=max)
//                                 .min_decimals(5)
//                                 .max_decimals(5)
//                                 .clamp_to_range(true)
//                                 .smart_aim(false)
//                                 .orientation(egui::SliderOrientation::Horizontal)
//                                 .text("m/z"),
//                         );

//                         let ppm_response = ui.add(
//                             Slider::new(&mut ppm, 0.0..=1000.0)
//                                 .clamp_to_range(true)
//                                 .smart_aim(false)
//                                 .orientation(egui::SliderOrientation::Horizontal)
//                                 .text("ppm"),
//                         );

//                         if mz_response.changed() || ppm_response.changed() {
//                             ui_state.last_mz_ppm = (mz, ppm);

//                             commands.entity(entity).insert(GenerateIonImage { mz, ppm });
//                         }

//                         // let opacity_response = ui.add(
//                         //     Slider::new(&mut ppm, 0.0..=100.0)
//                         //         .clamp_to_range(true)
//                         //         .smart_aim(true)
//                         //         .orientation(egui::SliderOrientation::Horizontal)
//                         //         .text("Opacity"),
//                         // );
//                         // if opacity_response.changed() {
//                         //     // Avoid double sending the event due to delay in event propagation
//                         //     ui_events.send(UiEvent::ImageEvent(ImageEvent::ChangeOpacity(
//                         //         control_entity,
//                         //         opacity_response,
//                         //     )));
//                         // }

//                         // Annotations
//                         // if let Some(children) = children {
//                         //     AnnotationGrid::new().ui(
//                         //         ui,
//                         //         children,
//                         //         &mut ui_events,
//                         //         &mut q_annotations,
//                         //     );
//                         // }

//                         let mut plot = spectrum::SpectrumPlot::new();

//                         for marker in q_marker.iter_mut() {
//                             if let Some(spectrum) = &marker.spectrum {
//                                 let mut spectrum: spectrum::Spectrum = spectrum.into();

//                                 spectrum.set_colour(marker.colour);
//                                 plot.add_spectrum(spectrum);
//                             }
//                         }

//                         ui.add(plot);

//                         // Spectrum markers
//                         egui::Grid::new("marker_grid")
//                             .num_columns(2)
//                             .spacing([40.0, 4.0])
//                             .striped(true)
//                             .show(ui, |ui| {
//                                 for mut marker in q_marker.iter_mut() {
//                                     ui.label(format!("Marker {:?}", marker.spectrum_id));
//                                     ui.color_edit_button_srgba(&mut marker.colour);
//                                     ui.end_row();
//                                 }
//                             });
//                     }
//                 });
//         });
// }

#[derive(Default, Component)]
pub struct Editable;

#[derive(Default, Component)]
pub struct Editing;

#[derive(WorldQuery)]
//#[world_query(mutable)]
struct UiControllable<'w> {
    ui_entry: &'w UiEntry,
    children: Option<&'w Children>,
    visibility: Option<&'w Visibility>,
    opacity: Option<&'w Opacity>,
    selectable: Option<&'w Selectable>,
    draggable: Option<&'w Draggable>,
    editable: Option<&'w Editable>,
}

// (
//     &UiEntry,
//     Option<&Children>,
//     Option<&Visibility>,
//     Option<&Sprite>,
// )

// struct  {

// }

fn add_children_to_ui_world(
    entity: Entity,
    ui: &mut Ui,
    world: &mut World,
    // q_ui_entry: &Query<UiControllable>,
    // ui_events: &mut EventWriter<UiEvent>,
    // ui_state: &UiState,
) {
    // egui::CollapsingHeader::new(format!("{}_{:?}", "ui_grid_for_", entity))
    // .num_columns(3)
    // .spacing([10.0, 4.0])
    // .striped(true)

    let children = world.get::<Children>(entity);
    if children.is_none() {
        return;
    }

    let children = children.unwrap().iter().copied().collect::<Vec<_>>();

    if !children.is_empty() {
        ui.add(Label::new("Children"));
    }

    // let mut q_ui_entry = world.query::<UiControllable>();
    let mut ui_events = Vec::new();

    for child in children.iter() {
        let description = world.get::<UiEntry>(*child).map(|s| s.description.clone());

        if let Some(description) = description {
            // Now decide what to do
            egui::CollapsingHeader::new(description.to_string()).show(ui, |ui| {
                ui.style_mut().spacing.button_padding = egui::Vec2::splat(1.0);

                // Add in information - e.g. number of cells
                if let Some(cell_segmentation) = world.get::<CellSegmentation>(*child) {
                    ui.label(format!("# cells: {}", cell_segmentation.num_cells));
                }

                let ui_state = world.get_resource::<UiState>().unwrap();

                ui.horizontal(|ui| {
                    let visibility = world.get::<Visibility>(*child);

                    if let Some(visibility) = visibility {
                        match visibility.is_visible {
                            true => {
                                let visibility_button = egui::ImageButton::new(
                                    ui_state.icon(UiIcon::Visible),
                                    egui::Vec2::splat(ui_state.icon_size),
                                );

                                if ui
                                    .add(visibility_button)
                                    .on_hover_text("Showing. Click to hide.")
                                    .clicked()
                                {
                                    ui_events.push(UiEvent::Image(ImageEvent::SetVisibility(
                                        *child,
                                        !visibility.is_visible,
                                    )));
                                }
                            }
                            false => {
                                let visibility_button = egui::ImageButton::new(
                                    ui_state.icon(UiIcon::NotVisible),
                                    egui::Vec2::splat(ui_state.icon_size),
                                );

                                if ui
                                    .add(visibility_button)
                                    .on_hover_text("Hiding annotation. Click to show.")
                                    .clicked()
                                {
                                    ui_events.push(UiEvent::Image(ImageEvent::SetVisibility(
                                        *child,
                                        !visibility.is_visible,
                                    )));
                                }
                            }
                        }
                    }

                    let opacity = world.get::<Opacity>(*child);
                    if let Some(opacity) = opacity {
                        let mut value = opacity.0;

                        let opacity = ui.add(
                            Slider::new(&mut value, 0.0..=1.0)
                                .step_by(0.01)
                                .clamp_to_range(true)
                                .orientation(egui::SliderOrientation::Horizontal)
                                .text("Opacity"),
                        );
                        if opacity.changed() {
                            // Avoid double sending the event due to delay in event propagation
                            ui_events.push(UiEvent::Image(ImageEvent::SetVisibility(*child, true)));
                            ui_events.push(UiEvent::Image(ImageEvent::SetOpacity(*child, value)));
                        }
                    }
                });

                let draggable = world.get::<Draggable>(*child);
                if draggable.is_some() {
                    ui.horizontal(|ui| {
                        if draggable.is_some() {
                            //ui.add(Label::new("Move"));

                            let selectable = world.get::<Selectable>(*child);

                            let button_title = if selectable.is_some() {
                                "Disable dragging"
                            } else {
                                "Enable dragging"
                            };

                            if ui.add(egui::Button::new(button_title)).clicked() {
                                ui_events.push(UiEvent::Image(ImageEvent::SetDragging(
                                    *child,
                                    selectable.is_none(),
                                )));
                            }
                        }

                        // if ui.add(egui::Button::new("Image alignment")).clicked() {
                        //     ui_events
                        //         .push(UiEvent::Image(ImageEvent::ToggleRegistration(*child, true)));
                        // }
                    });
                }

                // Check whether this is an acquisition, and if so, add in the ability to load a cell segmentation map
                let acquisition = world.get::<Acquisition>(*child);
                if acquisition.is_some() {
                    let open_button = egui::ImageButton::new(
                        ui_state.icon(UiIcon::FolderOpen),
                        egui::Vec2::splat(ui_state.icon_size),
                    );

                    ui.horizontal(|ui| {
                        ui.label("Cell segmentation");

                        if ui
                            .add(open_button)
                            .on_hover_text("Load cell segmentation image")
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Cell segmentation (.tif, .tiff)", &["tif", "tiff"])
                                .pick_file()
                            {
                                // self.picked_path = Some(path.display().to_string());
                                ui_events.push(UiEvent::Data(DataEvent::LoadCellData(*child, path)))
                            }
                        }
                    });
                }

                add_children_to_ui_world(*child, ui, world);
            });
        }
    }

    for event in ui_events {
        world.send_event(event);
    }
}

fn add_children_to_ui(
    entity: Entity,
    ui: &mut Ui,
    children: &Children,
    q_ui_entry: &Query<UiControllable>,
    ui_events: &mut EventWriter<UiEvent>,
    ui_state: &UiState,
) {
    // egui::CollapsingHeader::new(format!("{}_{:?}", "ui_grid_for_", entity))
    // .num_columns(3)
    // .spacing([10.0, 4.0])
    // .striped(true)

    if !children.is_empty() {
        ui.add(Label::new("Children"));
    }

    for child in children.iter() {
        if let Ok(ui_contollable) = q_ui_entry.get(*child) {
            // Now decide what to do
            egui::CollapsingHeader::new(ui_contollable.ui_entry.description.to_string()).show(
                ui,
                |ui| {
                    ui.style_mut().spacing.button_padding = egui::Vec2::splat(1.0);

                    ui.horizontal(|ui| {
                        if let Some(visibility) = ui_contollable.visibility {
                            match visibility.is_visible {
                                true => {
                                    let visibility_button = egui::ImageButton::new(
                                        ui_state.icon(UiIcon::Visible),
                                        egui::Vec2::splat(ui_state.icon_size),
                                    );

                                    if ui
                                        .add(visibility_button)
                                        .on_hover_text("Showing. Click to hide.")
                                        .clicked()
                                    {
                                        ui_events.send(UiEvent::Image(ImageEvent::SetVisibility(
                                            *child,
                                            !visibility.is_visible,
                                        )));
                                    }
                                }
                                false => {
                                    let visibility_button = egui::ImageButton::new(
                                        ui_state.icon(UiIcon::NotVisible),
                                        egui::Vec2::splat(ui_state.icon_size),
                                    );

                                    if ui
                                        .add(visibility_button)
                                        .on_hover_text("Hiding annotation. Click to show.")
                                        .clicked()
                                    {
                                        ui_events.send(UiEvent::Image(ImageEvent::SetVisibility(
                                            *child,
                                            !visibility.is_visible,
                                        )));
                                    }
                                }
                            }
                        }

                        if let Some(opacity) = ui_contollable.opacity {
                            let mut value = opacity.0;

                            let opacity = ui.add(
                                Slider::new(&mut value, 0.0..=1.0)
                                    .step_by(0.01)
                                    .clamp_to_range(true)
                                    .orientation(egui::SliderOrientation::Horizontal)
                                    .text("Opacity"),
                            );
                            if opacity.changed() {
                                // Avoid double sending the event due to delay in event propagation
                                ui_events
                                    .send(UiEvent::Image(ImageEvent::SetVisibility(*child, true)));
                                ui_events
                                    .send(UiEvent::Image(ImageEvent::SetOpacity(*child, value)));
                            }
                        }
                    });

                    if ui_contollable.draggable.is_some() {
                        ui.horizontal(|ui| {
                            if ui_contollable.draggable.is_some() {
                                //ui.add(Label::new("Move"));

                                let button_title = if ui_contollable.selectable.is_some() {
                                    "Disable dragging"
                                } else {
                                    "Enable dragging"
                                };

                                if ui.add(egui::Button::new(button_title)).clicked() {
                                    ui_events.send(UiEvent::Image(ImageEvent::SetDragging(
                                        *child,
                                        ui_contollable.selectable.is_none(),
                                    )));
                                }
                            }

                            if ui.add(egui::Button::new("Image alignment")).clicked() {
                                ui_events.send(UiEvent::Image(ImageEvent::ToggleRegistration(
                                    *child, true,
                                )));
                            }
                        });
                    }

                    if let Some(children) = ui_contollable.children {
                        add_children_to_ui(*child, ui, children, q_ui_entry, ui_events, ui_state);
                    }
                },
            );
        }
    }
}

fn imc_load_notification(mut egui_ctx: ResMut<EguiContext>, q_imc: Query<(Entity, &LoadIMC)>) {
    if !q_imc.is_empty() {
        egui::Window::new("Loading IMC data").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0 ,0.0)).show(egui_ctx.ctx_mut(), |ui| {
            ui.label("Loading IMC data. If this is the first time opening this dataset, this may take some time as a \
            cached version of the data is created for fast access to images. This is typically ~30% as large as the original data.");
        });
    }
}

fn ui_top_panel(
    mut egui_ctx: ResMut<EguiContext>,
    mut ui_events: EventWriter<UiEvent>,
    mut ui_space: ResMut<UiSpace>,
) {
    egui::TopBottomPanel::top("top_panel").show(egui_ctx.ctx_mut(), |ui| {
        let top_panel_size = ui.available_height() + 6.0;

        // Update the viewport stored if it is different.
        // Check is included here to avoid is_changed() being activated each frame
        if ui_space.ui_space.top != Val::Px(top_panel_size) {
            ui_space.ui_space.top = Val::Px(top_panel_size);
        }

        // The top panel is often a good place for a menu bar:
        egui::menu::bar(ui, |ui| {
            egui::menu::menu_button(ui, "File", |ui| {
                if ui.button("Open").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        //self.picked_path = Some(path.display().to_string());
                        ui_events.send(UiEvent::Data(DataEvent::OpenData(path)))
                    }
                }
                if ui.button("Quit").clicked() {
                    std::process::exit(0);
                }
            });

            egui::menu::menu_button(ui, "Annotations", |ui| {
                if ui.button("Import").clicked() {
                    let dialog = rfd::FileDialog::new()
                        .add_filter("Annotations", &["anno"])
                        .set_title("Select annotations to import");

                    if let Some(path) = dialog.pick_file() {
                        ui_events.send(UiEvent::Annotation(AnnotationEvent::Import(path)))
                    }
                }

                if ui.button("Export").clicked() {
                    let dialog = rfd::FileDialog::new()
                        .add_filter("Annotations", &["anno"])
                        .set_file_name("annotations.anno")
                        .set_title("Export annotations");

                    if let Some(path) = dialog.save_file() {
                        ui_events.send(UiEvent::Annotation(AnnotationEvent::Export {
                            annotations: None,
                            location: path,
                        }))
                    }
                }
            });
        });
    });
}

fn ui_bottom_panel(
    mut egui_ctx: ResMut<EguiContext>,
    q_mouse_position: Query<(&MousePosition, Option<&FieldOfView>)>,
    mut ui_space: ResMut<UiSpace>,
) {
    egui::TopBottomPanel::bottom("bottom_panel").show(egui_ctx.ctx_mut(), |ui| {
        let bottom_panel_height = ui.available_height() + 6.0;

        // Update the viewport stored if it is different.
        // Check is included here to avoid is_changed() being activated each frame
        if ui_space.ui_space.bottom != Val::Px(bottom_panel_height) {
            ui_space.ui_space.bottom = Val::Px(bottom_panel_height);
        }

        egui::menu::bar(ui, |ui| {
            if let Ok((mouse_position, field_of_view)) = q_mouse_position.get_single() {
                ui.label(format!(
                    "({}, {}) | From top left ({}, {})", // | Looking at {:?} - {:?}",
                    mouse_position.current_world.x,
                    mouse_position.current_world.y,
                    mouse_position.current_world.x,
                    25000.0 - mouse_position.current_world.y,
                    // field_of_view.top_left,
                    // field_of_view.bottom_right
                ));

                // ui.label(format!("Internal {:?}", mouse_position.current_window));
            }
        });
    });
}
