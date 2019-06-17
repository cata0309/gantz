use crate::node::{self, Node, SerdeNode};
use petgraph::visit::GraphBase;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::{Deref, DerefMut};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::FnArg;

pub mod codegen;

/// The type used to represent node and edge indices.
pub type Index = usize;

pub type EdgeIndex = petgraph::graph::EdgeIndex<Index>;
pub type NodeIndex = petgraph::graph::NodeIndex<Index>;

/// A trait required by graphs that support nesting graphs of the same type as nodes.
pub trait EvaluatorFnBlock {
    /// The `Evaluator` function block used to evaluate the graph from its inputs to its outputs.
    ///
    /// The function declaration is provided in order to allow the implementer to inspect the
    /// function inputs and output and create a function body accordingly.
    fn evaluator_fn_block(&self, fn_decl: &syn::FnDecl) -> syn::Block;
}

/// Describes a connection between two nodes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Edge {
    /// The output of the node at the source of this edge.
    pub output: node::Output,
    /// The input of the node at the destination of this edge.
    pub input: node::Input,
}

/// A node that itself is implemented in terms of a graph of nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GraphNode<G>
where
    G: GraphBase,
{
    /// The graph used to evaluate the node.
    pub graph: G,
    /// The types of each of the inputs into the graph node.
    ///
    /// TODO: Inlets and outlets should possibly use normal `Node`s and these should be their
    /// indices. This way we can retrieve the type from the graph, cast it to `Inlet`/`Outlet` and
    /// check for types while also allowing inlets and outlets to partake in the graph evaluation
    /// process.
    pub inlets: Vec<Inlet<G::NodeId>>,
    /// The types of each of the outputs into the graph node.
    pub outlets: Vec<Outlet<G::NodeId>>,
}

/// An inlet to a nested graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Inlet<Id> {
    /// The unique ID associated with this inlet's node in the graph.
    pub node_id: Id,
    /// The expected type for this inlet.
    #[serde(with = "crate::node::serde::ty")]
    pub ty: syn::Type,
}

/// An outlet from a nested graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Outlet<Id> {
    /// The unique ID associated with this outlet's node in the graph.
    pub node_id: Id,
    /// The expected type for this outlet.
    #[serde(with = "crate::node::serde::ty")]
    pub ty: syn::Type,
}

/// A node that may act as an inlet into a graph.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct InletNode;

/// A node that may act as an outlet from a graph.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct OutletNode;

/// The petgraph type used to represent a gantz graph.
pub type Graph<N> = petgraph::Graph<N, Edge, petgraph::Directed, Index>;

/// The petgraph type used to represent a stable gantz graph.
pub type StableGraph<N> = petgraph::stable_graph::StableGraph<N, Edge, petgraph::Directed, Index>;

impl Edge {
    /// Create an edge representing a connection from the given node `Output` to the given node
    /// `Input`.
    pub fn new(output: node::Output, input: node::Input) -> Self {
        Edge { output, input }
    }
}

impl<G> Node for GraphNode<G>
where
    G: GraphBase + EvaluatorFnBlock,
{
    fn evaluator(&self) -> node::Evaluator {
        let attrs = vec![];
        let vis = syn::Visibility::Inherited;
        let constness = None;
        let asyncness = None;
        let unsafety = None;
        let abi = None;
        // TODO: Make sure codegen makes the ident unique.
        // This will have to be considered in evaluator expr generation too.
        let name = format!("graph_node_evaluator_fn");
        let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
        let decl = Box::new(graph_node_evaluator_fn_decl(&self.inlets, &self.outlets));
        let block = Box::new(self.graph.evaluator_fn_block(&decl));
        let fn_item = syn::ItemFn {
            attrs,
            vis,
            constness,
            asyncness,
            unsafety,
            abi,
            ident,
            decl,
            block,
        };
        node::Evaluator::Fn { fn_item }
    }
}

// Manual implementation of `Deserialize` as it cannot be derived for a struct with associated
// types without unnecessary trait bounds on the struct itself.
impl<'de, G> Deserialize<'de> for GraphNode<G>
where
    G: GraphBase + Deserialize<'de>,
    G::NodeId: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, SeqAccess, Visitor};

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Graph,
            Inlets,
            Outlets,
        }

        struct GraphNodeVisitor<G>(std::marker::PhantomData<G>);

        impl<'de, G> Visitor<'de> for GraphNodeVisitor<G>
        where
            G: GraphBase + Deserialize<'de>,
            G::NodeId: Deserialize<'de>,
        {
            type Value = GraphNode<G>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct GraphNode")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<GraphNode<G>, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let graph = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let inlets = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let outlets = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                Ok(GraphNode {
                    graph,
                    inlets,
                    outlets,
                })
            }

            fn visit_map<V>(self, mut map: V) -> Result<GraphNode<G>, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut graph = None;
                let mut inlets = None;
                let mut outlets = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Graph => {
                            if graph.is_some() {
                                return Err(de::Error::duplicate_field("graph"));
                            }
                            graph = Some(map.next_value()?);
                        }
                        Field::Inlets => {
                            if inlets.is_some() {
                                return Err(de::Error::duplicate_field("inlets"));
                            }
                            inlets = Some(map.next_value()?);
                        }
                        Field::Outlets => {
                            if outlets.is_some() {
                                return Err(de::Error::duplicate_field("outlets"));
                            }
                            outlets = Some(map.next_value()?);
                        }
                    }
                }
                let graph = graph.ok_or_else(|| de::Error::missing_field("graph"))?;
                let inlets = inlets.ok_or_else(|| de::Error::missing_field("inlets"))?;
                let outlets = outlets.ok_or_else(|| de::Error::missing_field("outlets"))?;
                Ok(GraphNode {
                    graph,
                    inlets,
                    outlets,
                })
            }
        }

        const FIELDS: &[&str] = &["graph", "inlets", "outlets"];
        let visitor: GraphNodeVisitor<G> = GraphNodeVisitor(std::marker::PhantomData);
        deserializer.deserialize_struct("GraphNode", FIELDS, visitor)
    }
}

// Manual implementation of `Serialize` as it cannot be derived for a struct with associated
// types without unnecessary trait bounds on the struct itself.
impl<G> Serialize for GraphNode<G>
where
    G: GraphBase + Serialize,
    G::NodeId: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("GraphNode", 3)?;
        state.serialize_field("graph", &self.graph)?;
        state.serialize_field("inlets", &self.inlets)?;
        state.serialize_field("outlets", &self.outlets)?;
        state.end()
    }
}

impl Node for InletNode {
    fn evaluator(&self) -> node::Evaluator {
        let n_inputs = 1;
        let n_outputs = 1;
        //let ty = self.ty.clone();
        let gen_expr = Box::new(move |mut args: Vec<syn::Expr>| {
            assert_eq!(
                args.len(),
                1,
                "must be a single input (from the calling fn) for an inlet"
            );
            let in_expr = args.remove(0);
            syn::parse_quote! {
                //let in_expr_checked: #ty = #in_expr;
                //in_expr_checked
                #in_expr
            }
        });
        node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }
}

impl Node for OutletNode {
    fn evaluator(&self) -> node::Evaluator {
        let n_inputs = 1;
        let n_outputs = 1;
        //let ty = self.ty.clone();
        let gen_expr = Box::new(move |mut args: Vec<syn::Expr>| {
            assert_eq!(
                args.len(),
                1,
                "must be a single input (from the calling fn) for an inlet"
            );
            let out_expr = args.remove(0);
            syn::parse_quote! {
                //let out_expr_checked: #ty = #in_expr;
                //out_expr_checked
                #out_expr
            }
        });
        node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }
}

#[typetag::serde]
impl SerdeNode for InletNode {
    fn node(&self) -> &dyn Node {
        self
    }
}

#[typetag::serde]
impl SerdeNode for OutletNode {
    fn node(&self) -> &dyn Node {
        self
    }
}

impl<G> Deref for GraphNode<G>
where
    G: GraphBase,
{
    type Target = G;
    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

impl<G> DerefMut for GraphNode<G>
where
    G: GraphBase,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.graph
    }
}

impl<A, B> From<(A, B)> for Edge
where
    A: Into<node::Output>,
    B: Into<node::Input>,
{
    fn from((a, b): (A, B)) -> Self {
        let output = a.into();
        let input = b.into();
        Edge { output, input }
    }
}

fn graph_node_evaluator_fn_decl<Id>(inlets: &[Inlet<Id>], outlets: &[Outlet<Id>]) -> syn::FnDecl {
    let fn_token = syn::token::Fn {
        span: proc_macro2::Span::call_site(),
    };
    let generics = {
        // TODO: Eventually we'll want some way of inspecting inlets/outlets for these.
        let lt_token = None;
        let params = syn::punctuated::Punctuated::new();
        let gt_token = None;
        let where_clause = None;
        syn::Generics {
            lt_token,
            params,
            gt_token,
            where_clause,
        }
    };
    let paren_token = syn::token::Paren {
        span: proc_macro2::Span::call_site(),
    };
    let variadic = None;
    let inputs = graph_node_evaluator_fn_inputs(inlets);
    let output = graph_node_evaluator_fn_output(outlets);
    syn::FnDecl {
        fn_token,
        generics,
        paren_token,
        inputs,
        variadic,
        output,
    }
}

fn graph_node_evaluator_fn_inputs<Id>(inlets: &[Inlet<Id>]) -> Punctuated<FnArg, Comma> {
    inlets
        .iter()
        .enumerate()
        .map(|(i, inlet)| {
            let name = format!("inlet{}", i);
            let by_ref = None;
            let mutability = None;
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            let subpat = None;
            let pat_ident = syn::PatIdent {
                by_ref,
                mutability,
                ident,
                subpat,
            };
            let pat = pat_ident.into();
            let colon_token = Default::default();
            let ty = inlet.ty.clone();
            let arg_captured = syn::ArgCaptured {
                pat,
                colon_token,
                ty,
            };
            syn::FnArg::from(arg_captured)
        })
        .collect()
}

fn graph_node_evaluator_fn_output<Id>(outlets: &[Outlet<Id>]) -> syn::ReturnType {
    match outlets.len() {
        0 => syn::ReturnType::Default,
        1 => {
            let r_arrow = Default::default();
            let ty = Box::new(outlets[0].ty.clone());
            syn::ReturnType::Type(r_arrow, ty)
        }
        _ => {
            let paren_token = Default::default();
            let elems = outlets.iter().map(|outlet| outlet.ty.clone()).collect();
            let ty_tuple = syn::TypeTuple { paren_token, elems };
            let r_arrow = Default::default();
            let ty = Box::new(syn::Type::Tuple(ty_tuple));
            syn::ReturnType::Type(r_arrow, ty)
        }
    }
}
