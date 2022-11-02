use bevy::{math::Mat4, prelude::Transform};
use nalgebra::{DMatrix, Dim, Matrix4, VecStorage, Vector3, QR};

type TransformID = String;

#[derive(Debug)]
pub enum SpaceUnit {
    Pixels,
    Microns,
}

#[derive(Debug)]
pub struct AffineTransform {
    id: TransformID,

    pub(crate) matrix: Matrix4<f64>,
}

fn to_vector(points: Vec<Vector3<f64>>) -> DMatrix<f64> {
    let mut data: Vec<f64> = Vec::with_capacity(points.len() * 2);

    for point in &points {
        data.push(point.x);
        data.push(point.y);
    }

    let vec_storage = VecStorage::new(Dim::from_usize(points.len() * 2), Dim::from_usize(1), data);
    DMatrix::from_data(vec_storage)
}

// TODO: THIS IS TAKEN FROM imc_rs - we should try and avoid duplciating code like this
fn to_dmatrix(points: Vec<Vector3<f64>>) -> DMatrix<f64> {
    // TODO: At the moment we are ignoring the z as otherwise it generates a singular matrix
    let mut data: Vec<f64> = Vec::with_capacity(points.len() * 12);

    for point in &points {
        data.push(point.x);
        data.push(point.y);
        data.push(1.0);
        data.extend((0..6).map(|_x| 0.0));
        data.push(point.x);
        data.push(point.y);
        data.push(1.0);
    }

    println!("Data {:?}", data);

    let vec_storage = VecStorage::new(Dim::from_usize(6), Dim::from_usize(points.len() * 2), data);
    DMatrix::from_data(vec_storage).transpose()
}

impl AffineTransform {
    pub fn new(id: TransformID, matrix: Matrix4<f64>) -> Self {
        Self { id, matrix }
    }

    pub fn from_scale(id: TransformID, scale_x: f64, scale_y: f64) -> Self {
        let mut matrix = Matrix4::identity();
        matrix.m11 = scale_x;
        matrix.m22 = scale_y;

        Self { id, matrix }
    }

    pub fn from_points(
        id: TransformID,
        fixed_points: Vec<Vector3<f64>>,
        moving_points: Vec<Vector3<f64>>,
    ) -> Self {
        let fixed = to_dmatrix(moving_points);
        let moving = to_vector(fixed_points);

        // println!("FIXED = {}", fixed);
        // println!("MOVING = {}", moving);

        let qr = QR::new(fixed);
        // println!("q = {}", qr.q());
        // println!("r = {}", qr.r());
        let qt_r = qr.q().transpose() * moving;
        // println!("qt_r = {}", qt_r);
        //let res = qr.solve(&b);
        let r_t = qr.r().try_inverse().unwrap();
        // println!("r_t = {}", r_t);

        let res = r_t * qt_r;

        // println!("RESULT = {}", res);

        // Probably a better way to do this
        // Copy data from the solution to linear equations into Matrix4
        let mut matrix = Matrix4::identity();
        matrix.m11 = *res.get(0).unwrap();
        matrix.m12 = *res.get(1).unwrap();
        matrix.m14 = *res.get(2).unwrap();
        matrix.m21 = *res.get(3).unwrap();
        matrix.m22 = *res.get(4).unwrap();
        matrix.m24 = *res.get(5).unwrap();

        Self { id, matrix }
    }

    pub fn id(&self) -> &TransformID {
        &self.id
    }

    pub fn scale(mut self, x_scale: f64, y_scale: f64, z_scale: f64) -> Self {
        let mut matrix = Matrix4::identity();
        matrix.m11 = x_scale;
        matrix.m22 = y_scale;
        matrix.m33 = z_scale;

        // TODO: It is this way around to handle input from .regi - THIS SHOULD BE CHANGED
        self.matrix = matrix * self.matrix;

        self
    }

    pub fn translate_x(mut self, x: f64) -> Self {
        self.matrix.m14 += x;

        self
    }

    pub fn translate_y(mut self, x: f64) -> Self {
        self.matrix.m24 += x;

        self
    }
}

impl From<&AffineTransform> for Mat4 {
    fn from(transform: &AffineTransform) -> Mat4 {
        Mat4::from_cols_array(&[
            transform.matrix.m11 as f32,
            transform.matrix.m12 as f32,
            transform.matrix.m13 as f32,
            transform.matrix.m14 as f32,
            transform.matrix.m21 as f32,
            transform.matrix.m22 as f32,
            transform.matrix.m23 as f32,
            transform.matrix.m24 as f32,
            transform.matrix.m31 as f32,
            transform.matrix.m32 as f32,
            transform.matrix.m33 as f32,
            transform.matrix.m34 as f32,
            transform.matrix.m41 as f32,
            transform.matrix.m42 as f32,
            transform.matrix.m43 as f32,
            transform.matrix.m44 as f32,
        ])
        .transpose()
    }
}

impl From<AffineTransform> for Mat4 {
    fn from(transform: AffineTransform) -> Mat4 {
        (&transform).into()
    }
}

impl From<&AffineTransform> for Transform {
    fn from(transform: &AffineTransform) -> Self {
        Transform::from_matrix(transform.into())
    }
}

impl From<AffineTransform> for Transform {
    fn from(transform: AffineTransform) -> Self {
        Transform::from_matrix((&transform).into())
    }
}
