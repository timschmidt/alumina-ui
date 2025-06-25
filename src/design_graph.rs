use egui_node_graph2::*;
use csgrs::csg::CSG;
use nalgebra::{Vector3};
use egui::{self, DragValue};

#[derive(Clone, Debug)]
pub struct EmptyUserResponse;

impl UserResponseTrait for EmptyUserResponse {}

/// What kinds of data can flow through the graph?
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum DType {
    Csg,
    Scalar,
    Vec3,
}

/// Run-time value carried by a port when the graph is evaluated.
#[derive(Clone, Debug)]
pub enum DValue {
    Csg(CSG<()>),
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
    // primitives
    Cube,
    Sphere,
    Cylinder,
    // booleans
    Union,
    Subtract,
    Intersect,
    // transforms
    Translate,
    Rotate,
    Scale,
}

impl Default for Template {
    fn default() -> Self {
        Template::Cube        // pick whichever variant you like
    }
}

#[derive(Default)]
pub struct UserState; // we do not need per-graph global state (yet)

#[derive(Default)]
pub struct NodeData {
    pub template: Template,
}

/// Make the three “typeclasses” requested by egui-node-graph:
impl DataTypeTrait<UserState> for DType {
    fn data_type_color(&self, _u: &mut UserState) -> egui::Color32 {
        match self {
            DType::Csg   => egui::Color32::from_rgb(110, 200, 255),
            DType::Scalar => egui::Color32::from_rgb(38, 109, 211),
            DType::Vec3  => egui::Color32::from_rgb(238, 207, 109),
        }
    }
    fn name(&self) -> std::borrow::Cow<'_, str> {
        match self {
            DType::Csg   => "solid".into(),
            DType::Scalar => "scalar".into(),
            DType::Vec3  => "vec3".into(),
        }
    }
}

impl NodeTemplateTrait for Template {
    type NodeData = NodeData;
    type DataType = DType;
    type ValueType = DValue;
    type UserState = UserState;
    type CategoryType = &'static str;

    fn node_finder_label(&self, _: &mut UserState) -> std::borrow::Cow<'_, str> {
        use Template::*;
        match self {
            Cube          => "Cube".into(),
            Sphere        => "Sphere".into(),
            Cylinder      => "Cylinder".into(),
            Union         => "Union".into(),
            Subtract      => "Subtract".into(),
            Intersect     => "Intersect".into(),
            Translate     => "Translate".into(),
            Rotate        => "Rotate".into(),
            Scale         => "Scale".into(),
        }
    }

    fn node_finder_categories(&self, _: &mut UserState) -> Vec<Self::CategoryType> {
        use Template::*;
        match self {
            Cube|Sphere|Cylinder          => vec!["Primitives"],
            Union|Subtract|Intersect      => vec!["Boolean"],
            Translate|Rotate|Scale        => vec!["Transform"],
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
        let csg_in    = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_input_param(
                id,
                name.into(),
                DType::Csg,
                DValue::default(),
                InputParamKind::ConnectionOnly,
                true,
            );
        };
        let csg_out   = |g: &mut Graph<NodeData, DType, DValue>, name: &str| {
            g.add_output_param(id, name.into(), DType::Csg);
        };

        use Template::*;
        match self {
            Cube => {
                scalar_in(g, "size");
                csg_out(g, "out");
            }
            Sphere => {
                scalar_in(g, "radius");
                csg_out(g, "out");
            }
            Cylinder => {
                scalar_in(g, "radius");
                scalar_in(g, "height");
                csg_out(g, "out");
            }
            Union | Subtract | Intersect => {
                csg_in(g, "A");
                csg_in(g, "B");
                csg_out(g, "out");
            }
            Translate => {
                csg_in(g, "in");
                vec3_in(g, "offset");
                csg_out(g, "out");
            }
            Rotate => {
                csg_in(g, "in");
                vec3_in(g, "axis");
                scalar_in(g, "angle (rad)");
                csg_out(g, "out");
            }
            Scale => {
                csg_in(g, "in");
                vec3_in(g, "factors");
                csg_out(g, "out");
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
            Cube, Sphere, Cylinder,
            Union, Subtract, Intersect,
            Translate, Rotate, Scale,
        ]
    }
}

/// We draw scalars/vec3 widgets exactly like in the sample
impl WidgetValueTrait for DValue {
    type NodeData   = NodeData;
    type Response   = EmptyUserResponse;
    type UserState  = UserState;

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
            DValue::Csg(_) => { ui.label("solid"); }
        }
        Vec::new()
    }
}

impl NodeDataTrait for NodeData {
    type Response   = EmptyUserResponse;
    type UserState  = UserState;
    type DataType   = DType;
    type ValueType  = DValue;

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

pub fn evaluate(
    graph: &GraphT,
    root: OutputId,           // evaluate “this” output
) -> anyhow::Result<CSG<()>> {
    let mut cache = Cache::new();
    let val = eval_rec(graph, root, &mut cache)?;
    match val {
        DValue::Csg(csg) => Ok(csg),
        _ => anyhow::bail!("root output does not evaluate to a solid"),
    }
}

fn eval_rec(graph: &GraphT, out: OutputId, cache: &mut Cache) -> anyhow::Result<DValue> {
    if let Some(v) = cache.get(&out) { return Ok(v.clone()); }
    let node_id = graph[out].node;
    let node    = &graph[node_id];
    use Template::*;
    let mut get = |name: &str| -> anyhow::Result<DValue> {
        let in_id = node.get_input(name)?;
        if let Some(src) = graph.connection(in_id) {
            eval_rec(graph, src, cache)
        } else {
            Ok(graph[in_id].value.clone())
        }
    };

    let value = match node.user_data.template {
        Cube => {
            let size = get("size")?.scalar()?;
            DValue::Csg(CSG::cube(size, None))
        }
        Sphere => {
            let r = get("radius")?.scalar()?;
            DValue::Csg(CSG::sphere(r, 24, 24, None))
        }
        Cylinder => {
            let r = get("radius")?.scalar()?;
            let h = get("height")?.scalar()?;
            DValue::Csg(CSG::cylinder(r, h, 24, None))
        }
        Union => {
            let a = get("A")?.csg()?;
            let b = get("B")?.csg()?;
            DValue::Csg(a.union(&b))
        }
        Subtract => {
            let a = get("A")?.csg()?;
            let b = get("B")?.csg()?;
            DValue::Csg(a.difference(&b))
        }
        Intersect => {
            let a = get("A")?.csg()?;
            let b = get("B")?.csg()?;
            DValue::Csg(a.intersection(&b))
        }
        Translate => {
            let s = get("in")?.csg()?;
            let o = get("offset")?.vec3()?;
            DValue::Csg(s.translate(o.x.into(), o.y.into(), o.z.into()))
        }
        Rotate => {
			let s    = get("in")?.csg()?;
			let axis = get("axis")?.vec3()?.normalize();
			let ang  = get("angle (rad)")?.scalar()?;
			let deg  = ang.to_degrees();
			DValue::Csg(
				s.rotate(axis.x * deg, axis.y * deg, axis.z * deg)
			)
		}
        Scale => {
            let s = get("in")?.csg()?;
            let f = get("factors")?.vec3()?;
            DValue::Csg(s.scale(f.x.into(), f.y.into(), f.z.into()))
        }
    };

    cache.insert(out, value.clone());
    Ok(value)
}

// -- tiny helpers ----------------------------------------------------

trait AsTyped {
    fn scalar(self) -> anyhow::Result<f32>;
    fn vec3(self)   -> anyhow::Result<Vector3<f32>>;
    fn csg(self)    -> anyhow::Result<CSG<()>>;
}
impl AsTyped for DValue {
    fn scalar(self) -> anyhow::Result<f32> {
        if let DValue::Scalar(x) = self { Ok(x) } else { anyhow::bail!("expected scalar") }
    }
    fn vec3(self) -> anyhow::Result<Vector3<f32>> {
        if let DValue::Vec3(v) = self { Ok(v) } else { anyhow::bail!("expected vec3") }
    }
    fn csg(self) -> anyhow::Result<CSG<()>> {
        if let DValue::Csg(c) = self { Ok(c) } else { anyhow::bail!("expected solid") }
    }
}
