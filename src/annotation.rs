use std::{
    fs::File,
    io::{self, BufReader, BufWriter},
    path::{Path, PathBuf},
};

use bevy::{math::DVec2, prelude::*};
use bevy_egui::{egui::Color32, EguiContext};
use bevy_prototype_lyon::prelude::{
    DrawMode, FillMode, GeometryBuilder, PathBuilder, StrokeMode, StrokeOptions,
};

use geo_booleanop::boolean::BooleanOp;
use geo_types::{LineString, MultiPolygon, Polygon};
use serde::{Deserialize, Serialize};

use crate::{camera::MousePosition, ui::Editing, Message};

/// AnnotationPlugin
///
/// This includes all events and systems required to add, edit, remove, load and save annotations.
///
/// The easiest way to interact with this plugin is via an [`AnnotationEvent`]. This plugin will
/// consume all [`AnnotationEvent`]s present at each frame and issue the respective commands.
pub struct AnnotationPlugin;

impl Plugin for AnnotationPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<AnnotationEvent>()
            .add_startup_system(load_autosaved_annotations)
            // .add_system(hide_children_annotations)
            .add_system(handle_annotation_event)
            .add_system(edit_annotation)
            .add_system(update_annotation)
            .add_system(annotation_hint)
            .add_system(annotation_hint_update);
    }
}

/// Annotation events define ways to interact with this plugin.
#[derive(Clone)]
pub enum AnnotationEvent {
    /// Add a new annotation with the given name and colour.
    Add {
        /// Name of the annotation.
        name: String,
        /// Representative colour for the annotation.
        colour: Color32,
    },
    /// Remove the annotation with the given [`Entity`].
    Remove(Entity),
    /// Show the annotation with the given [`Entity`]. If the annotation is currently not visible,
    /// then it will be made visible.
    Show(Entity),
    /// Hide the annotation with the given [`Entity`]. If the annotation is currently visible,
    /// then it will be made not visible.
    Hide(Entity),
    /// Allow editing of the annotation with the given [`Entity`].
    ///
    /// This adds the [`Editing`] component to this annotation, so that this can be detected by other
    /// systems, e.g. the UI.
    ///
    /// Only one annotation can be edited at one point in time, so [`Editing`] component is removed from all
    /// other annotations.
    Edit(Entity),
    /// Set the active editing tool for the annotation with the given [`Entity`].
    SetActiveTool {
        /// Annotation whose editing tool should be altered.
        entity: Entity,
        /// The tool which should now be used for editing the annotation (or [`None`] if no tool should be active).
        active_tool: Option<Tool>,
    },
    /// Stop editing annotations. This removes the [`Editing`] component from all annotations.
    StopEdit,
    /// Set the colour of the annotation with the given [`Entity`].
    SetColour {
        /// Annotation whose colour should be altered.
        entity: Entity,
        /// New representative colour to use for this annotation.
        colour: Color32,
    },
    /// Set the description of the annotation with the given [`Entity`].
    SetDescription {
        /// Annotation whose description should be altered.
        entity: Entity,
        /// New description for this annotation.
        description: String,
    },
    /// Export all annotations to JSON at the specified location.
    Export {
        /// Location to save the annotations to.
        location: PathBuf,
        /// Optionally a set of annotations to save. If None, all will be saved
        annotations: Option<Vec<Entity>>,
    },
    /// Import annotations from a previously saved JSON file at the specified location.
    Import(PathBuf),
}

/// Handle annotation events
fn handle_annotation_event(
    mut commands: Commands,
    mut ev_annotation: EventReader<AnnotationEvent>,
    mut q_annotations: Query<(Entity, &mut Annotation, Option<&Children>, Option<&Editing>)>,
    mut q_visibility: Query<&mut Visibility>,
    mut q_draw_mode: Query<&mut DrawMode>,
    q_annotation_hints: Query<Entity, With<AnnotationHint>>,
) {
    for event in ev_annotation.iter() {
        match event {
            AnnotationEvent::Add { name, colour } => {
                commands
                    .spawn_bundle(SpatialBundle::default())
                    .insert(Annotation::with_egui_colour(name, *colour));
            }
            AnnotationEvent::Remove(entity) => {
                commands.entity(*entity).despawn_recursive();
            }
            AnnotationEvent::Hide(entity) => {
                // Now check for children
                if let Ok((_, _, _, editing)) = q_annotations.get(*entity) {
                    if editing.is_some() {
                        // If we are editing, we shouldn't be able to hide the annotation
                        continue;
                    }

                    if let Ok(mut visibility) = q_visibility.get_mut(*entity) {
                        visibility.is_visible = false;
                    }

                    // if let Some(children) = children {
                    //     for child in children.iter() {
                    //         if let Ok(mut visibility) = q_visibility.get_mut(*child) {
                    //             visibility.is_visible = false;
                    //         }
                    //     }
                    // }
                }
            }
            AnnotationEvent::Show(entity) => {
                if let Ok(mut visibility) = q_visibility.get_mut(*entity) {
                    visibility.is_visible = true;
                }

                // Now check for children
                // if let Ok((_, _, Some(children), _)) = q_annotations.get(*entity) {
                //     for child in children.iter() {
                //         if let Ok(mut visibility) = q_visibility.get_mut(*child) {
                //             visibility.is_visible = true;
                //         }
                //     }
                // }
            }
            AnnotationEvent::Edit(entity) => {
                // Can only edit one annotation at a time
                for (annotation_entity, mut annotation, _, editing) in q_annotations.iter_mut() {
                    if editing.is_some() {
                        if annotation_entity != *entity {
                            commands.entity(annotation_entity).remove::<Editing>();
                        }
                    } else if *entity == annotation_entity {
                        commands.entity(annotation_entity).insert(Editing);

                        annotation.active_tool = Some(Tool::Pencil { radius: 20.0 });
                    }
                }
            }
            AnnotationEvent::StopEdit => {
                // Remove editing hints
                for hint in q_annotation_hints.iter() {
                    commands.entity(hint).despawn_recursive();
                }

                for (annotation_entity, mut annotation, _, editing) in q_annotations.iter_mut() {
                    if editing.is_some() {
                        commands.entity(annotation_entity).remove::<Editing>();

                        annotation.active_tool = None;
                    }
                }

                // Autosave the annotations for next time
                let annotations: Vec<Annotation> = q_annotations
                    .iter()
                    .map(|(_, annotation, _, _)| annotation.clone())
                    .collect();

                if let Err(error) = save_annotations("autosave_annotations.json", &annotations) {
                    commands.spawn(Message::from(error));
                }
            }
            AnnotationEvent::SetActiveTool {
                entity,
                active_tool,
            } => {
                if let Ok((_, mut annotation, _, _)) = q_annotations.get_mut(*entity) {
                    if let Some(Tool::Pencil { radius }) = active_tool {
                        annotation.outline = *radius / 10.0;
                    }

                    annotation.active_tool = *active_tool
                }
            }
            AnnotationEvent::SetColour { entity, colour } => {
                if let Ok((_, mut annotation, children, _)) = q_annotations.get_mut(*entity) {
                    annotation.colour = *colour;

                    if let Some(children) = children {
                        for child in children.iter() {
                            if let Ok(mut draw_mode) = q_draw_mode.get_mut(*child) {
                                match draw_mode.as_mut() {
                                    DrawMode::Fill(fill) => fill.color = annotation.bevy_colour(),
                                    DrawMode::Stroke(stroke) => {
                                        stroke.color = annotation.bevy_colour()
                                    }
                                    DrawMode::Outlined {
                                        fill_mode,
                                        outline_mode,
                                    } => {
                                        let mut colour = annotation.bevy_colour();
                                        colour.set_a(1.0);

                                        fill_mode.color = annotation.bevy_colour();
                                        outline_mode.color = colour;
                                    }
                                }
                            }
                        }
                    }
                }

                // Autosave the annotations for next time
                let annotations: Vec<Annotation> = q_annotations
                    .iter()
                    .map(|(_, annotation, _, _)| annotation.clone())
                    .collect();

                if let Err(error) = save_annotations("autosave_annotations.json", &annotations) {
                    commands.spawn(Message::from(error));
                }
            }
            AnnotationEvent::Import(path) => match File::open(path) {
                Ok(file) => {
                    let reader = BufReader::new(file);
                    match serde_json::from_reader::<BufReader<File>, Vec<Annotation>>(reader) {
                        Ok(annotations) => {
                            for annotation in annotations {
                                commands.spawn((
                                    annotation,
                                    Visibility { is_visible: true },
                                    Transform::default(),
                                    GlobalTransform::default(),
                                ));
                            }
                        }
                        Err(error) => {
                            commands.spawn(Message::from(AnnotationError::from(error)));
                        }
                    };
                }
                Err(error) => {
                    commands.spawn(Message::from(AnnotationError::from(error)));
                }
            },
            AnnotationEvent::Export {
                annotations,
                location,
            } => {
                let mut to_save = Vec::new();

                // Make sure that the correct extension is set
                let mut location = location.clone();
                location.set_extension("anno");

                if let Some(annotations) = annotations {
                    for ann_entity in annotations {
                        if let Ok((_, annotation, _, _)) = q_annotations.get(*ann_entity) {
                            to_save.push(annotation.clone());
                        }
                    }
                } else {
                    for (_, annotation, _, _) in q_annotations.iter() {
                        to_save.push(annotation.clone());
                    }
                }

                if let Err(error) = save_annotations(location, &to_save) {
                    commands.spawn(Message::from(error));
                }
            }
            AnnotationEvent::SetDescription {
                entity,
                description,
            } => {
                if let Ok((_, mut annotation, _, _)) = q_annotations.get_mut(*entity) {
                    annotation.description = description.to_string();
                }
            }
        }
    }
}

/// For testing - Annotations are automatically saved to the file `autosave_annotations.json` located in the
/// same folder as the application. This startup system automatically reloads the previously saved annotations.
/// No check is made to ensure that these annotations are appropriate for the loaded data.
fn load_autosaved_annotations(mut commands: Commands) {
    // Check to see if old annotations are present
    if let Ok(file) = File::open("autosave_annotations.json") {
        let reader = BufReader::new(file);
        let annotations: Vec<Annotation> = match serde_json::from_reader(reader) {
            Ok(annotations) => annotations,
            Err(error) => {
                commands.spawn(Message::from(AnnotationError::from(error)));

                return;
            }
        };

        for annotation in annotations {
            commands.spawn((annotation, SpatialBundle::default()));
        }
    }
}

/// Helper function for converting a Vec4 to a Vec2, by ignoring z and w components.
#[inline]
fn vec4_to_vec2(vec: Vec4) -> Vec2 {
    Vec2::new(vec.x, vec.y)
}

enum AnnotationError {
    IoError(io::Error),
    SerdeJsonError(serde_json::Error),
}

impl From<AnnotationError> for Message {
    fn from(error: AnnotationError) -> Self {
        Self {
            severity: crate::Severity::Error,
            message: error.to_string(),
        }
    }
}

impl ToString for AnnotationError {
    fn to_string(&self) -> String {
        match self {
            AnnotationError::IoError(error) => error.to_string(),
            AnnotationError::SerdeJsonError(error) => error.to_string(),
        }
    }
}

impl From<io::Error> for AnnotationError {
    fn from(error: io::Error) -> Self {
        AnnotationError::IoError(error)
    }
}
impl From<serde_json::Error> for AnnotationError {
    fn from(error: serde_json::Error) -> Self {
        AnnotationError::SerdeJsonError(error)
    }
}

fn save_annotations<P: AsRef<Path>>(
    location: P,
    annotations: &Vec<Annotation>,
) -> Result<(), AnnotationError> {
    let file = File::create(location)?;
    let writer = BufWriter::new(file);

    serde_json::to_writer(writer, annotations)?;

    Ok(())
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Tool {
    Pencil { radius: f32 },
    Rubber { radius: f32 },
    Polygon {},
}

pub struct PixelAnnotationConf<'s> {
    pub width: u32,
    pub height: u32,

    pub transform: &'s GlobalTransform,
}

#[derive(Component, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub(crate) description: String,
    pub(crate) colour: Color32,

    outline: f32,
    //pub(crate) annotation_type: AnnotationType,
    polygon: MultiPolygon<f64>,

    active_tool: Option<Tool>,
    last_pixel: Option<Vec2>,

    // We shouldn't (de-)serialise this as the entity will be different at the next run
    #[serde(skip)]
    editing_camera: Option<Entity>,
}

impl Annotation {
    pub fn with_egui_colour(description: &str, colour: Color32) -> Self {
        Self {
            description: description.to_string(),
            colour,
            outline: 5.0,
            polygon: MultiPolygon::new(vec![]),
            active_tool: None,
            last_pixel: None,
            editing_camera: None,
        }
    }

    pub fn with_bevy_colour(description: &str, colour: Color) -> Self {
        Self {
            description: description.to_string(),
            colour: Color32::from_rgba_premultiplied(
                (colour.r() * 255.0) as u8,
                (colour.g() * 255.0) as u8,
                (colour.b() * 255.0) as u8,
                (colour.a() * 255.0) as u8,
            ),
            outline: 5.0,
            polygon: MultiPolygon::new(vec![]),
            active_tool: None,
            last_pixel: None,
            editing_camera: None,
        }
    }

    pub fn bevy_colour(&self) -> Color {
        Color::rgba_u8(
            self.colour.r(),
            self.colour.g(),
            self.colour.b(),
            self.colour.a(),
        )
    }

    pub fn egui_colour(&self) -> Color32 {
        self.colour
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn active_tool(&self) -> Option<Tool> {
        self.active_tool
    }

    pub fn pixel_annotation(
        &self,
        conf: &PixelAnnotationConf,
        from: (u32, u32),
        to: (u32, u32),
        pixels: &mut Vec<(u32, u32)>,
    ) {
        let half_width = conf.width as f32 / 2.0;
        let half_height = conf.height as f32 / 2.0;

        let top_left = conf.transform.transform_point(Vec3::new(
            from.0 as f32 - half_width,
            from.1 as f32 - half_height,
            1.0,
        ));

        let top_right = conf.transform.transform_point(Vec3::new(
            to.0 as f32 - half_width,
            from.1 as f32 - half_height,
            1.0,
        ));

        let bottom_right = conf.transform.transform_point(Vec3::new(
            to.0 as f32 - half_width,
            to.1 as f32 - half_height,
            1.0,
        ));

        let bottom_left = conf.transform.transform_point(Vec3::new(
            from.0 as f32 - half_width,
            to.1 as f32 - half_height,
            1.0,
        ));

        let coords = vec![
            (top_left.x as f64, top_left.y as f64),
            (top_right.x as f64, top_right.y as f64),
            (bottom_right.x as f64, bottom_right.y as f64),
            (bottom_left.x as f64, bottom_left.y as f64),
        ];

        // println!("{} {}", top_left, bottom_right);
        // println!("{:?}", coords);
        let area_to_check = MultiPolygon::new(vec![Polygon::new(LineString::from(coords), vec![])]);

        let result = area_to_check.difference(&self.polygon);

        //print!("Result [{:?} | {:?}] {}", from, to, result.0.len());

        // if !result.0.is_empty() {
        //     println!(
        //         ": {:?} | {}",
        //         result.0[0].exterior().0.len(),
        //         result.0[0].interiors().len()
        //     )
        // } else {
        //     println!();
        // }

        if result.0.is_empty() {
            //println!("Regions were equal!");

            for y in from.1..to.1 {
                for x in from.0..to.0 {
                    pixels.push((x, y));
                }
            }
        } else if result.0.len() == 1
            && result.0[0].interiors().is_empty()
            && result.0[0].exterior().0.len() == area_to_check.0[0].exterior().0.len()
        {
            // TODO: Check whether the result is really the same as the area to check
        } else {
            //if result.0.len() > 1 || !result.0[0].interiors().is_empty() {
            // Continue splitting
            let mid_x = (to.0 - from.0) / 2 + from.0;
            let mid_y = (to.1 - from.1) / 2 + from.1;

            // Make sure that we are actually going to process something new
            if mid_x != from.0 {
                if mid_x > from.0 && mid_y > from.1 {
                    self.pixel_annotation(conf, from, (mid_x, mid_y), pixels);
                }
                // Top right quadrant
                if to.0 > mid_x && mid_y > from.1 {
                    self.pixel_annotation(conf, (mid_x, from.1), (to.0, mid_y), pixels);
                }
            }

            if mid_y != from.1 {
                // Bottom left quadrant
                if mid_x > (from.0) && to.1 > (mid_y) {
                    self.pixel_annotation(conf, (from.0, mid_y), (mid_x, to.1), pixels);
                }

                // Bottom right quadrant
                if to.0 > (mid_x) && to.1 > (mid_y) {
                    self.pixel_annotation(conf, (mid_x, mid_y), to, pixels);
                }
            }
            // } else {
            //     if result.0[0].exterior().0.len() == area_to_check.0[0].exterior().0.len() {
            //         // Check whether the result is the same as the area to check
            //     } else {
            // println!("Stopping: {:?}", result.0[0].exterior())
            // }
        }
        //if result
    }
}

#[derive(Debug)]
struct Line(DVec2, DVec2);

impl Line {
    fn to_polygon(&self, radius: f64) -> Vec<DVec2> {
        let mut points = Vec::new();

        let direction = self.1 - self.0;
        let normal = DVec2::new(direction.y, -direction.x).normalize_or_zero();

        points.push(self.0 + normal * radius);
        points.push(self.1 + normal * radius);

        let num_points = 10;
        let start_angle = std::f64::consts::PI * 2.0 - normal.angle_between(DVec2::new(1.0, 0.0));
        let angle = std::f64::consts::PI / num_points as f64;

        for i in 1..num_points {
            // println!("{:?}", (angle * i as f32));
            points.push(
                self.1
                    + DVec2::new(
                        radius * (start_angle + angle * i as f64).cos(),
                        radius * (start_angle + angle * i as f64).sin(),
                    ),
            );
        }

        // points.push(self.1);

        points.push(self.1 + normal * -radius);
        points.push(self.0 + normal * -radius);

        let start_angle = std::f64::consts::PI - normal.angle_between(DVec2::new(1.0, 0.0));

        for i in 1..num_points {
            points.push(
                self.0
                    + DVec2::new(
                        radius * (start_angle + angle * i as f64).cos(),
                        radius * (start_angle + angle * i as f64).sin(),
                    ),
            );
        }

        points
    }
}

fn circle(center: DVec2, radius: f64) -> Vec<DVec2> {
    let mut points = Vec::new();

    let num_points = 20;
    let start_angle = 0.0;
    let angle = std::f64::consts::PI * 2.0 / num_points as f64;

    for i in 0..num_points {
        // println!("{:?}", (angle * i as f32));
        points.push(
            center
                + DVec2::new(
                    radius * (start_angle + angle * i as f64).cos(),
                    radius * (start_angle + angle * i as f64).sin(),
                ),
        );
    }

    points
}

fn edit_annotation(
    mut egui_ctx: ResMut<EguiContext>,
    mouse_input: Res<Input<MouseButton>>,
    q_mouse_position: Query<&MousePosition>,
    mut q_annotation: Query<(Entity, &mut Annotation), With<Editing>>,
) {
    // Check position is not in the menu or side panel
    if egui_ctx.ctx_mut().wants_keyboard_input()
        || egui_ctx.ctx_mut().is_pointer_over_area()
        || egui_ctx.ctx_mut().wants_pointer_input()
        || egui_ctx.ctx_mut().is_using_pointer()
    {
        if let Ok((_, mut annotation)) = q_annotation.get_single_mut() {
            if annotation.last_pixel.is_some() {
                annotation.last_pixel = None;
            }
        }

        return;
    }

    let mouse_position = q_mouse_position.single();

    // Make sure we are editing one thing and only one thing
    if let Ok((_, mut annotation)) = q_annotation.get_single_mut() {
        // Check that we are in the correct viewport
        if let (Some(mouse_camera), Some(edit_camera)) =
            (mouse_position.active_camera, annotation.editing_camera)
        {
            if mouse_camera != edit_camera {
                return;
            }
        }

        if let Some(active_tool) = &annotation.active_tool {
            match active_tool {
                Tool::Pencil { radius } => {
                    // If the mouse has just changed to be in a different viewport, we should stop editing the annotation
                    if mouse_input.just_released(MouseButton::Left) {
                        annotation.last_pixel = None;
                    } else if mouse_input.pressed(MouseButton::Left) {
                        let new_point = vec4_to_vec2(mouse_position.current_world);

                        match annotation.last_pixel {
                            Some(last_pix) => {
                                if last_pix.distance(new_point).abs() > *radius {
                                    let line = Line(
                                        DVec2::new(last_pix.x as f64, last_pix.y as f64),
                                        DVec2::new(new_point.x as f64, new_point.y as f64),
                                    );
                                    let line_polygon = line.to_polygon(*radius as f64);

                                    let mut line_string_vec = Vec::new();

                                    for point in line_polygon {
                                        line_string_vec.push((point.x, point.y))
                                    }

                                    let line_polygon =
                                        Polygon::new(LineString::from(line_string_vec), vec![]);

                                    annotation.polygon = annotation.polygon.union(&line_polygon);
                                    annotation.last_pixel = Some(new_point);

                                    true
                                } else {
                                    false
                                }
                            }
                            None => {
                                let line_polygon = circle(
                                    DVec2::new(new_point.x as f64, new_point.y as f64),
                                    *radius as f64,
                                );
                                let mut line_string_vec = Vec::new();

                                for point in line_polygon {
                                    line_string_vec.push((point.x, point.y))
                                }

                                let circle_polygon =
                                    Polygon::new(LineString::from(line_string_vec), vec![]);

                                annotation.last_pixel = Some(new_point);
                                annotation.editing_camera = mouse_position.active_camera;
                                annotation.polygon = annotation.polygon.union(&circle_polygon);

                                true
                            }
                        };
                    }
                }
                Tool::Rubber { radius } => todo!(),
                Tool::Polygon {} => todo!(),
            }
        }
    }
}

fn update_annotation(
    mut commands: Commands,
    q_annotation: Query<(Entity, &Annotation), Changed<Annotation>>,
) {
    for (entity, annotation) in q_annotation.iter() {
        // println!("Changed");
        commands.entity(entity).despawn_descendants();
        commands.entity(entity).with_children(|parent| {
            for polygon in &annotation.polygon.0 {
                let mut builder = PathBuilder::new();
                let points = polygon.exterior();

                let first_point = points.0[0];
                builder.move_to(Vec2::new(first_point.x as f32, first_point.y as f32));

                for point in points.0.iter().skip(1) {
                    builder.line_to(Vec2::new(point.x as f32, point.y as f32));
                }

                for points in polygon.interiors() {
                    let first_point = points.0[0];
                    builder.move_to(Vec2::new(first_point.x as f32, first_point.y as f32));

                    for point in points.0.iter().skip(1) {
                        builder.line_to(Vec2::new(point.x as f32, point.y as f32));
                    }
                }

                let path = builder.build();

                let mut colour = annotation.bevy_colour();
                colour.set_a(1.0);

                parent.spawn(GeometryBuilder::build_as(
                    &path,
                    DrawMode::Outlined {
                        fill_mode: FillMode::color(annotation.bevy_colour()),
                        outline_mode: StrokeMode {
                            options: StrokeOptions::default().with_line_width(annotation.outline),
                            color: colour, //Color::BLACK,
                        },
                    },
                    Transform::from_xyz(0., 0., 10.0),
                ));
            }
        });
    }
}

#[derive(Component)]
struct AnnotationHint {
    annotation: Entity,
}

fn annotation_hint(
    mut commands: Commands,
    q_editing: Query<(Entity, &Annotation), Or<(Changed<Annotation>, Added<Editing>)>>,
    q_hints: Query<Entity, With<AnnotationHint>>,
    mouse_position: Query<&MousePosition>,
) {
    let mouse_position = mouse_position.single();

    for (entity, annotation) in q_editing.iter() {
        // Remove old hint
        for hint in q_hints.iter() {
            commands.entity(hint).despawn_recursive();
        }

        let current_world = vec4_to_vec2(mouse_position.current_world);

        if let Some(active_tool) = &annotation.active_tool {
            match active_tool {
                Tool::Pencil { radius } => {
                    let mut builder = PathBuilder::new();
                    builder.move_to(Vec2::new(*radius, 0.0));
                    builder.arc(
                        Vec2::splat(0.0),
                        Vec2::splat(*radius),
                        2.0 * std::f32::consts::PI,
                        0.0,
                    );
                    let path = builder.build();

                    let mut colour = annotation.bevy_colour();
                    colour.set_a(0.25);

                    commands.spawn((
                        GeometryBuilder::build_as(
                            &path,
                            DrawMode::Outlined {
                                fill_mode: FillMode::color(colour),
                                outline_mode: StrokeMode {
                                    options: StrokeOptions::default()
                                        .with_line_width(radius / 20.0),
                                    color: annotation.bevy_colour(), //Color::BLACK,
                                },
                            },
                            Transform::from_xyz(current_world.x, current_world.y, 100.0),
                        ),
                        AnnotationHint { annotation: entity },
                    ));
                }
                Tool::Rubber { radius } => todo!(),
                Tool::Polygon {} => todo!(),
            }
        }
    }
}

fn annotation_hint_update(
    mut q_hint: Query<&mut Transform, With<AnnotationHint>>,
    mouse_position: Query<&MousePosition>,
) {
    let mouse_position = mouse_position.single();

    for mut transform in q_hint.iter_mut() {
        let current_world = vec4_to_vec2(mouse_position.current_world);

        transform.translation.x = current_world.x;
        transform.translation.y = current_world.y;
    }
}
