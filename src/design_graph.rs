use egui_node_graph2::*;
use csgrs::{mesh::Mesh, sketch::Sketch, traits::CSG};
use nalgebra::Vector3;
use egui::{self, DragValue};

#[derive(Clone, Debug)]
pub struct EmptyUserResponse;

impl UserResponseTrait for EmptyUserResponse {}

/// Ports may carry scalars, vectors, planar **sketches**, or volumetric **meshes**.
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum DType {
    Mesh,
    Sketch,
    Scalar,
    Vec3,
}

/// Run-time value carried by a port when the graph is evaluated.
#[derive(Clone, Debug)]
pub enum DValue {
    Mesh(Mesh<()>),
    Sketch(Sketch<()>),
    Scalar(f32),
    Vec3(Vector3<f32>),
}

impl Default for DValue {
    fn default() -> Self {
        Self::Scalar(0.0)
    }
}

/// A node “template” = what appears in the “add node” pop-up.
#[derive(Copy, Clone)]
pub enum Template {
    // 3-D primitives
    Cube,
    Sphere,
    Cylinder,
    // 2-D primitives
    Rectangle,
    Circle,
    // Booleans (mesh ↔ mesh  OR sketch ↔ sketch)
    Union,
    Subtract,
    Intersect,
    // transforms
    Translate,
    Rotate,
    Scale,
    // 2-D → 3-D
    Extrude,
}

impl Default for Template {
    fn default() -> Self {
        Template::Cube
    }
}

// Helper: list “root” output sockets of the graph (no consumer attached).
pub fn graph_roots(graph: &GraphT) -> Vec<OutputId> {
    use std::collections::HashSet;
    let mut used = HashSet::<OutputId>::new();
    for input_id in graph.inputs.keys() {
        if let Some(src) = graph.connection(input_id) {
            used.insert(src);
        }
    }
    graph
        .outputs
        .keys()
        .filter(|oid| !used.contains(oid))
        .collect()
}

#[derive(Default)]
pub struct UserState;

#[derive(Default)]
pub struct NodeData {
    pub template: Template,
}

/// Color & label palette for sockets
impl DataTypeTrait<UserState> for DType {
    fn data_type_color(&self, _u: &mut UserState) -> egui::Color32 {
        match self {
            DType::Mesh => egui::Color32::from_rgb(110, 200, 255),
            DType::Sketch => egui::Color32::from_rgb(120, 180, 120),
            DType::Scalar => egui::Color32::from_rgb(38, 109, 211),
            DType::Vec3 => egui::Color32::from_rgb(238, 207, 109),
        }
    }
    fn name(&self) -> std::borrow::Cow<'_, str> {
        match self {
            DType::Mesh => "mesh".into(),
            DType::Sketch => "sketch".into(),
            DType::Scalar => "scalar".into(),
            DType::Vec3 => "vec3".into(),
        }
    }
}

/// Node-template plumbing ----------------------------------------------------
impl NodeTemplateTrait for Template {
    type NodeData = NodeData;
    type DataType = DType;
    type ValueType = DValue;
    type UserState = UserState;
    type CategoryType = &'static str;

    fn node_finder_label(&self, _: &mut UserState) -> std::borrow::Cow<'_, str> {
        use Template::*;
        match self {
            Cube        => "Cube".into(),
            Sphere      => "Sphere".into(),
            Cylinder    => "Cylinder".into(),
            Rectangle   => "Rectangle".into(),
            Circle      => "Circle".into(),
            Union       => "Union".into(),
            Subtract    => "Subtract".into(),
            Intersect   => "Intersect".into(),
            Translate   => "Translate".into(),
            Rotate      => "Rotate".into(),
            Scale       => "Scale".into(),
            Extrude     => "Extrude".into(),
        }
    }

    fn node_finder_categories(&self, _: &mut UserState) -> Vec<Self::CategoryType> {
        use Template::*;
        match self {
            Cube | Sphere | Cylinder => vec!["3-D / Mesh"],
            Rectangle | Circle => vec!["2-D / Sketch"],
            Union | Subtract | Intersect => vec!["Boolean"],
            Translate | Rotate | Scale => vec!["Transform"],
            Extrude => vec!["2-D → 3-D"],
        }
    }

    fn node_graph_label(&self, u: &mut UserState) -> String {
        self.node_finder_label(u).into()
    }

    fn user_data(&self, _: &mut UserState) -> Self::NodeData {
        NodeData { template: *self }
    }

    fn build_node(
        &self,
        g: &mut Graph<NodeData, DType, DValue>,
        _: &mut UserState,
        id: NodeId,
    ) {
        let scalar_in = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_input_param(
                id,
                name.into(),
                DType::Scalar,
                DValue::Scalar(1.0),
                InputParamKind::ConnectionOrConstant,
                true,
            );
        };
        let vec3_in   = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_input_param(
                id,
                name.into(),
                DType::Vec3,
                DValue::Vec3(Vector3::zeros()),
                InputParamKind::ConnectionOrConstant,
                true,
            );
        };
        let mesh_in = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
             g.add_input_param(
                id,
                name.into(),
                DType::Mesh,
                DValue::default(),
                InputParamKind::ConnectionOnly,
                true,
            );
        };
	let sketch_in = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_input_param(
                id,
                name.into(),
                DType::Sketch,
                DValue::default(),
                InputParamKind::ConnectionOnly,
                true,
            );
        };
        let mesh_out = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_output_param(id, name.into(), DType::Mesh);
        };
        let sketch_out = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_output_param(id, name.into(), DType::Sketch);
        };

        //—-- build socket layout -------------------------------------------
        use Template::*;
        match self {
            // 3-D
            Cube => {
                scalar_in(g, "size");
                mesh_out(g, "out");
            }
            Sphere => {
                scalar_in(g, "radius");
                mesh_out(g, "out");
            }
            Cylinder => {
                scalar_in(g, "radius");
                scalar_in(g, "height");
                mesh_out(g, "out");
            }
            // 2-D
            Rectangle => {
                scalar_in(g, "width");
                scalar_in(g, "height");
                sketch_out(g, "out");
            }
            Circle => {
                scalar_in(g, "radius");
                sketch_out(g, "out");
            }
            // Boolean (works for both kinds, use same DType for A/B)
            Union | Subtract | Intersect => {
                mesh_in(g, "A");
                mesh_in(g, "B");
                mesh_out(g, "out");
            }
            // Transforms
            Translate => {
                mesh_in(g, "in");
                vec3_in(g, "offset");
                mesh_out(g, "out");
            }
            Rotate => {
                mesh_in(g, "in");
                vec3_in(g, "axis");
                scalar_in(g, "angle (rad)");
                mesh_out(g, "out");
            }
            Scale => {
                mesh_in(g, "in");
                vec3_in(g, "factors");
                mesh_out(g, "out");
            }
            // 2-D → 3-D
            Extrude => {
                sketch_in(g, "profile");
                scalar_in(g, "height");
                mesh_out(g, "out");
            }
        }
    }
}

/// Tell egui-node-graph which templates exist
pub struct AllTemplates;
impl NodeTemplateIter for AllTemplates {
    type Item = Template;
    fn all_kinds(&self) -> Vec<Self::Item> {
        use Template::*;
        vec![
            Cube, Sphere, Cylinder, Rectangle, Circle, Union, Subtract, Intersect, Translate,
            Rotate, Scale, Extrude,
        ]
    }
}

/// We draw scalars/vec3 widgets exactly like in the sample
impl WidgetValueTrait for DValue {
    type NodeData = NodeData;
    type Response = EmptyUserResponse;
    type UserState = UserState;

    fn value_widget(
        &mut self,
        name: &str,
        _node_id: NodeId,
        ui: &mut egui::Ui,
        _user_state: &mut UserState,
        _node_data: &NodeData,
    ) -> Vec<Self::Response> {
        match self {
            DValue::Scalar(x) => {
                ui.horizontal(|ui| {
                    ui.label(name);
                    ui.add(DragValue::new(x));
                });
            }
            DValue::Vec3(v) => {
                ui.label(name);
                ui.horizontal(|ui| {
                    ui.label("x"); ui.add(DragValue::new(&mut v.x));
                    ui.label("y"); ui.add(DragValue::new(&mut v.y));
                    ui.label("z"); ui.add(DragValue::new(&mut v.z));
                });
            }
            DValue::Sketch(_) => {
                ui.label("sketch");
            }
            DValue::Mesh(_) => {
                ui.label("mesh");
            }
        }
        Vec::new()
    }
}

/// Only bottom-panel UI (none here)
impl NodeDataTrait for NodeData {
    type Response = EmptyUserResponse;
    type UserState = UserState;
    type DataType = DType;
    type ValueType = DValue;

    fn bottom_ui(
        &self,
        _ui: &mut egui::Ui,
        _id: NodeId,
        _graph: &Graph<NodeData, DType, DValue>,
        _state: &mut UserState,
    ) -> Vec<NodeResponse<EmptyUserResponse, Self>> {
        Vec::new()
    }
}

// ---------- evaluation ----------------------------------------------------------------------------------

type Cache = std::collections::HashMap<OutputId, DValue>;
type GraphT = Graph<NodeData, DType, DValue>;

/// --------------------------------------------------------------------------
/// **Graph evaluation** – returns a 3-D `Mesh<()>` to display.
pub fn evaluate(graph: &GraphT, root: OutputId) -> anyhow::Result<Mesh<()>> {
    let mut cache = Cache::new();
    let val = eval_rec(graph, root, &mut cache)?;
    match val {
        DValue::Mesh(mesh) => Ok(mesh),
        _ => anyhow::bail!("root output does not evaluate to a mesh"),
    }
}

fn eval_rec(graph: &GraphT, out: OutputId, cache: &mut Cache) -> anyhow::Result<DValue> {
    if let Some(v) = cache.get(&out) { return Ok(v.clone()); }
    let node_id = graph[out].node;
    let node    = &graph[node_id];

    // Helper to fetch (recursively) an input
    let mut get = |name: &str| -> anyhow::Result<DValue> {
        let in_id = node.get_input(name)?;
        if let Some(src) = graph.connection(in_id) {
            eval_rec(graph, src, cache)
        } else {
            Ok(graph[in_id].value.clone())
        }
    };

    use Template::*;
    let value = match node.user_data.template {
        // 3-D primitives ----------------------------------------------------
        Cube => {
            let size = get("size")?.scalar()?;
            DValue::Mesh(Mesh::cube(size.into(), None))
        }
        Sphere => {
            let r = get("radius")?.scalar()?;
            DValue::Mesh(Mesh::sphere(r.into(), 24, 24, None))
        }
        Cylinder => {
            let r = get("radius")?.scalar()?;
            let h = get("height")?.scalar()?;
            DValue::Mesh(Mesh::cylinder(r.into(), h.into(), 24, None))
        }

        // 2-D primitives ----------------------------------------------------
        Rectangle => {
            let w = get("width")?.scalar()?;
            let h = get("height")?.scalar()?;
            DValue::Sketch(Sketch::rectangle(w.into(), h.into(), None))
        }
        Circle => {
            let r = get("radius")?.scalar()?;
            DValue::Sketch(Sketch::circle(r.into(), 64, None))
        }

        // Boolean (mesh) ----------------------------------------------------
        Union => {
            let a = get("A")?.mesh()?;
            let b = get("B")?.mesh()?;
            DValue::Mesh(a.union(&b))
        }
        Subtract => {
            let a = get("A")?.mesh()?;
            let b = get("B")?.mesh()?;
            DValue::Mesh(a.difference(&b))
        }
        Intersect => {
            let a = get("A")?.mesh()?;
            let b = get("B")?.mesh()?;
            DValue::Mesh(a.intersection(&b))
        }

        // Transforms ------------------------------------------------------------
        Translate => {
            let m = get("in")?.mesh()?;
            let o = get("offset")?.vec3()?;
            DValue::Mesh(m.translate(o.x.into(), o.y.into(), o.z.into()))
        }
        Rotate => {
            let m = get("in")?.mesh()?;
            let axis = get("axis")?.vec3()?.normalize();
            let ang = get("angle (rad)")?.scalar()?;
            let deg = ang.to_degrees();
            DValue::Mesh(m.rotate(axis.x * deg, axis.y * deg, axis.z * deg))
        }
        Scale => {
            let m = get("in")?.mesh()?;
            let f = get("factors")?.vec3()?;
            DValue::Mesh(m.scale(f.x.into(), f.y.into(), f.z.into()))
        }

        // 2-D → 3-D ---------------------------------------------------------
        Extrude => {
            let s = get("profile")?.sketch()?;
            let h = get("height")?.scalar()?;
            DValue::Mesh(s.extrude(h.into()))
        }
    };

    cache.insert(out, value.clone());
    Ok(value)
}

/// Small helpers for type-safe extraction -----------------------------------
trait AsTyped {
    fn scalar(self) -> anyhow::Result<f32>;
    fn vec3(self) -> anyhow::Result<Vector3<f32>>;
    fn mesh(self) -> anyhow::Result<Mesh<()>>;
    fn sketch(self) -> anyhow::Result<Sketch<()>>;
}
impl AsTyped for DValue {
    fn scalar(self) -> anyhow::Result<f32> {
        if let DValue::Scalar(x) = self {
            Ok(x)
        } else {
            anyhow::bail!("expected scalar")
        }
    }
    fn vec3(self) -> anyhow::Result<Vector3<f32>> {
        if let DValue::Vec3(v) = self {
            Ok(v)
        } else {
            anyhow::bail!("expected vec3")
        }
    }
    fn mesh(self) -> anyhow::Result<Mesh<()>> {
        if let DValue::Mesh(m) = self {
            Ok(m)
        } else {
            anyhow::bail!("expected mesh")
        }
    }
    fn sketch(self) -> anyhow::Result<Sketch<()>> {
        if let DValue::Sketch(s) = self {
            Ok(s)
        } else {
            anyhow::bail!("expected sketch")
        }
    }
}
