use crate::node::{self, Node, SerdeNode};
use petgraph::visit::{Data, GraphBase};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::{Deref, DerefMut};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::FnArg;

pub mod codegen;

/// Required by graphs that support nesting graphs of the same type as nodes.
pub trait EvaluatorFnBlock: GraphBase {
    /// The `Evaluator` function block used to evaluate the graph from its inputs to its outputs.
    ///
    /// The function declaration is provided in order to allow the implementer to inspect the
    /// function inputs and output and create a function body accordingly.
    fn evaluator_fn_block(
        &self,
        inlets: &[Self::NodeId],
        outlets: &[Self::NodeId],
        signature: &syn::Signature,
    ) -> syn::Block;
}

/// Types that may be used as a graph within a **GraphNode**.
pub trait Graph: EvaluatorFnBlock {
    /// The node type used within the inner graph.
    type Node: Node;
    /// Return a reference to the node at the given node ID.
    fn node(&self, id: Self::NodeId) -> Option<&Self::Node>;
    /// The state type associated with the graph accessible and updatable during evaluation.
    ///
    /// The produced type should probably provide access to the state of the graph's inner nodes.
    ///
    /// This method is used to determine the `Node::state_type` result within the implementation of
    /// `Node` for `GraphNode`.
    fn state_type(&self) -> syn::Type;
}

/// A trait implemented for graph types capable of adding nodes and returning a unique ID
/// associated with the added node.
///
/// This trait allows `gantz` to provide the `GraphNode::add_inlet` and `add_outlet` methods.
pub trait AddNode: Data {
    /// Add a node with the given weight and return its unique ID.
    fn add_node(&mut self, n: Self::NodeWeight) -> Self::NodeId;
}

/// The name of the function generated for performing full evaluation of the graph.
pub const FULL_EVAL_FN_NAME: &str = "full_eval";

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
    pub inlets: Vec<G::NodeId>,
    /// The types of each of the outputs into the graph node.
    pub outlets: Vec<G::NodeId>,
}

/// An inlet to a nested graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Inlet {
    /// The expected type for this inlet.
    #[serde(with = "crate::node::serde::ty")]
    pub ty: syn::Type,
}

/// An outlet from a nested graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Outlet {
    /// The expected type for this outlet.
    #[serde(with = "crate::node::serde::ty")]
    pub ty: syn::Type,
}

impl Edge {
    /// Create an edge representing a connection from the given node `Output` to the given node
    /// `Input`.
    pub fn new(output: node::Output, input: node::Input) -> Self {
        Edge { output, input }
    }
}

impl Inlet {
    /// Construct an inlet with the given type.
    pub fn new(ty: syn::Type) -> Self {
        Inlet { ty }
    }

    /// The same as `new` but parses the type from the given `str`.
    pub fn parse(ty: &str) -> syn::Result<Self> {
        Ok(Self::new(syn::parse_str(ty)?))
    }
}

impl Outlet {
    /// Construct an outlet with the given type.
    pub fn new(ty: syn::Type) -> Self {
        Outlet { ty }
    }

    /// The same as `new` but parses the type from the given `str`.
    pub fn parse(ty: &str) -> syn::Result<Self> {
        Ok(Self::new(syn::parse_str(ty)?))
    }
}

impl<G> GraphNode<G>
where
    G: AddNode,
{
    /// Adds the given `NodeId` to the graph as an inlet node.
    ///
    /// This is the same as `G::add_node`, but also adds the resulting node index to the
    /// `GraphNode`'s `inlets` list.
    pub fn add_inlet(&mut self, n: G::NodeWeight) -> G::NodeId {
        let id = self.add_node(n);
        self.inlets.push(id);
        id
    }

    /// Adds the given `NodeId` to the graph as an outlet node.
    ///
    /// This is the same as `G::add_node`, but also adds the resulting node index to the
    /// `GraphNode`'s `outlet` list.
    pub fn add_outlet(&mut self, n: G::NodeWeight) -> G::NodeId {
        let id = self.add_node(n);
        self.outlets.push(id);
        id
    }
}

impl<'a, T> EvaluatorFnBlock for &'a T
where
    T: EvaluatorFnBlock,
{
    fn evaluator_fn_block(
        &self,
        inlets: &[Self::NodeId],
        outlets: &[Self::NodeId],
        signature: &syn::Signature,
    ) -> syn::Block {
        (*self).evaluator_fn_block(inlets, outlets, signature)
    }
}

impl<'a, T> Graph for &'a T
where
    T: Graph,
{
    type Node = T::Node;
    fn node(&self, id: Self::NodeId) -> Option<&Self::Node> {
        (*self).node(id)
    }
    fn state_type(&self) -> syn::Type {
        (*self).state_type()
    }
}

impl<G> Node for GraphNode<G>
where
    G: Graph,
{
    fn evaluator(&self) -> node::Evaluator {
        let attrs = vec![];
        let vis = syn::Visibility::Inherited;
        let state_ty = Graph::state_type(&self.graph);
        let mut sig = Box::new(graph_node_evaluator_signature(
            &self.graph,
            &state_ty,
            &self.inlets,
            &self.outlets,
        ));
        let block = Box::new(
            self.graph
                .evaluator_fn_block(&self.inlets, &self.outlets, &sig),
        );
        sig.inputs.pop();
        let fn_item = syn::ItemFn {
            attrs,
            vis,
            sig: *sig,
            block,
        };
        node::Evaluator::Fn { fn_item }
    }

    fn state_type(&self) -> Option<syn::Type> {
        Some(Graph::state_type(&self.graph))
    }
}

impl<G> Default for GraphNode<G>
where
    G: GraphBase + Default,
{
    fn default() -> Self {
        let graph = Default::default();
        let inlets = Default::default();
        let outlets = Default::default();
        GraphNode {
            graph,
            inlets,
            outlets,
        }
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

impl Node for Inlet {
    fn evaluator(&self) -> node::Evaluator {
        let n_inputs = 0;
        let n_outputs = 1;
        let gen_expr = Box::new(move |args: Vec<syn::Expr>| {
            assert!(
                args.is_empty(),
                "there cannot be any inputs to an inlet node"
            );
            syn::parse_quote! {{
                state.clone()
            }}
        });
        node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }

    fn state_type(&self) -> Option<syn::Type> {
        Some(self.ty.clone())
    }
}

impl Node for Outlet {
    fn evaluator(&self) -> node::Evaluator {
        let n_inputs = 1;
        let n_outputs = 0;
        let ty = self.ty.clone();
        let gen_expr = Box::new(move |mut args: Vec<syn::Expr>| {
            assert_eq!(args.len(), 1, "must be a single input for an outlet");
            let in_expr = args.remove(0);
            syn::parse_quote! {{
                let state: &mut #ty = state;
                *state = #in_expr;
            }}
        });
        node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }

    fn state_type(&self) -> Option<syn::Type> {
        Some(self.ty.clone())
    }
}

impl<N, E, Ty, Ix> AddNode for petgraph::Graph<N, E, Ty, Ix>
where
    Ty: petgraph::EdgeType,
    Ix: petgraph::graph::IndexType,
{
    fn add_node(&mut self, n: N) -> petgraph::graph::NodeIndex<Ix> {
        petgraph::Graph::add_node(self, n)
    }
}

impl<N, E, Ty, Ix> AddNode for petgraph::stable_graph::StableGraph<N, E, Ty, Ix>
where
    Ty: petgraph::EdgeType,
    Ix: petgraph::graph::IndexType,
{
    fn add_node(&mut self, n: N) -> petgraph::graph::NodeIndex<Ix> {
        petgraph::stable_graph::StableGraph::add_node(self, n)
    }
}

#[typetag::serde]
impl SerdeNode for Inlet {
    fn node(&self) -> &dyn Node {
        self
    }
}

#[typetag::serde]
impl SerdeNode for Outlet {
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

/// The identifier used for graph full eval functions.
pub fn full_eval_fn_ident() -> syn::Ident {
    syn::Ident::new(FULL_EVAL_FN_NAME, proc_macro2::Span::call_site())
}

/// The function signature for performing full evaluation of a graph.
///
/// A `full_eval_fn` is generated once for every nested graph that contains one or more inlets or
/// outlets.
pub fn full_eval_fn() -> node::EvalFn {
    let ident = full_eval_fn_ident();
    let item_fn: syn::ItemFn = syn::parse_quote! {
        fn #ident() {}
    };
    item_fn.into()
}

fn graph_node_evaluator_signature<G>(
    g: G,
    state_ty: &syn::Type,
    inlets: &[G::NodeId],
    outlets: &[G::NodeId],
) -> syn::Signature
where
    G: Graph,
{
    let constness = None;
    let asyncness = None;
    let unsafety = None;
    let abi = None;
    let fn_token = syn::token::Fn::default();
    // TODO: Make sure codegen makes the ident unique.
    // This will have to be considered in evaluator expr generation too.
    let name = format!("graph_node_evaluator_fn");
    let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
    // TODO: Eventually we'll want some way of inspecting inlets/outlets for these.
    let generics = syn::Generics::default();
    let paren_token = syn::token::Paren::default();
    let variadic = None;
    let state_arg = graph_node_evaluator_fn_state_arg(state_ty);
    let mut inputs = graph_node_evaluator_fn_inputs(&g, inlets);
    inputs.push(state_arg);
    let output = graph_node_evaluator_fn_output(&g, outlets);
    syn::Signature {
        constness,
        asyncness,
        unsafety,
        abi,
        fn_token,
        ident,
        generics,
        paren_token,
        inputs,
        variadic,
        output,
    }
}

fn expect_node_state_type<G>(g: G, n: G::NodeId) -> syn::Type
where
    G: Graph,
{
    g.node(n)
        .expect("no node for the given id")
        .state_type()
        .expect("no state type for node at id")
}

fn graph_node_evaluator_fn_state_arg(state_ty: &syn::Type) -> FnArg {
    let name = "state";
    let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
    let fn_arg = syn::parse_quote! { #ident: #state_ty };
    fn_arg
}

fn graph_node_evaluator_fn_inputs<G>(g: G, inlets: &[G::NodeId]) -> Punctuated<FnArg, Comma>
where
    G: Graph,
{
    inlets
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            let name = format!("inlet{}", i);
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            let ty = expect_node_state_type(&g, n);
            let fn_arg: syn::FnArg = syn::parse_quote! { #ident: #ty };
            fn_arg
        })
        .collect()
}

fn graph_node_evaluator_fn_output<G>(g: G, outlets: &[G::NodeId]) -> syn::ReturnType
where
    G: Graph,
{
    match outlets.len() {
        0 => syn::ReturnType::Default,
        1 => {
            let r_arrow = Default::default();
            let ty = Box::new(expect_node_state_type(g, outlets[0]));
            syn::ReturnType::Type(r_arrow, ty)
        }
        _ => {
            let paren_token = Default::default();
            let elems = outlets
                .iter()
                .map(|&id| expect_node_state_type(&g, id))
                .collect();
            let ty_tuple = syn::TypeTuple { paren_token, elems };
            let r_arrow = Default::default();
            let ty = Box::new(syn::Type::Tuple(ty_tuple));
            syn::ReturnType::Type(r_arrow, ty)
        }
    }
}
