#[macro_use]
extern crate serde;

#[derive(Deserialize, Serialize)]
struct One;

#[derive(Deserialize, Serialize)]
struct Add;

#[derive(Deserialize, Serialize)]
struct Debug;

impl gantz::Node for One {
    fn evaluator(&self) -> gantz::node::Evaluator {
        let n_inputs = 0;
        let n_outputs = 1;
        let gen_expr = Box::new(|_| syn::parse_quote! { 1 });
        gantz::node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }

    fn push_eval(&self) -> Option<gantz::node::EvalFn> {
        let item_fn: syn::ItemFn = syn::parse_quote! { fn one_push_eval() {} };
        Some(item_fn.into())
    }
}

impl gantz::Node for Add {
    fn evaluator(&self) -> gantz::node::Evaluator {
        let n_inputs = 2;
        let n_outputs = 1;
        let gen_expr = Box::new(move |args: Vec<syn::Expr>| {
            let l = &args[0];
            let r = &args[1];
            syn::parse_quote! { #l + #r }
        });
        gantz::node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }
}

impl gantz::Node for Debug {
    fn evaluator(&self) -> gantz::node::Evaluator {
        let n_inputs = 1;
        let n_outputs = 0;
        let gen_expr = Box::new(move |args: Vec<syn::Expr>| {
            assert_eq!(args.len(), 1);
            let input = &args[0];
            syn::parse_quote! { println!("{:?}", #input) }
        });
        gantz::node::Evaluator::Expr {
            n_inputs,
            n_outputs,
            gen_expr,
        }
    }
}

#[typetag::serde]
impl gantz::node::SerdeNode for One {
    fn node(&self) -> &dyn gantz::Node {
        self
    }
}

#[typetag::serde]
impl gantz::node::SerdeNode for Add {
    fn node(&self) -> &dyn gantz::Node {
        self
    }
}

#[typetag::serde]
impl gantz::node::SerdeNode for Debug {
    fn node(&self) -> &dyn gantz::Node {
        self
    }
}

fn main() {
    // Create a project called `foo` in `./examples/foo`
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("foo");
    let mut project = gantz::Project::open(path.into()).unwrap();

    // Instantiate the core nodes.
    let one = Box::new(One) as Box<dyn gantz::node::SerdeNode>;
    let add = Box::new(Add) as Box<_>;
    let debug = Box::new(Debug) as Box<_>;

    // Add nodes to the project.
    let one = project.add_core_node(one);
    let add = project.add_core_node(add);
    let debug = project.add_core_node(debug);

    // Update the root graph.
    let root = project.root_node_id();
    project
        .update_graph(&root, |g| {
            let one = g.add_node(one);
            let add = g.add_node(add);
            let debug = g.add_node(debug);
            g.add_edge(
                one,
                add,
                gantz::Edge {
                    output: gantz::node::Output(0),
                    input: gantz::node::Input(0),
                },
            );
            g.add_edge(
                one,
                add,
                gantz::Edge {
                    output: gantz::node::Output(0),
                    input: gantz::node::Input(1),
                },
            );
            g.add_edge(
                add,
                debug,
                gantz::Edge {
                    output: gantz::node::Output(0),
                    input: gantz::node::Input(0),
                },
            );
        })
        .unwrap();

    // Retrieve the path to the compiled library.
    let dylib_path = project
        .graph_node_dylib(&root)
        .unwrap()
        .expect("no dylib or node");
    let lib = libloading::Library::new(&dylib_path).expect("failed to load library");
    let symbol_name = "one_push_eval".as_bytes();
    unsafe {
        let foo_one_push_eval_fn: libloading::Symbol<fn(&mut [&mut dyn std::any::Any])> =
            lib.get(symbol_name).expect("failed to load symbol");
        // Execute the gantz graph (prints `2` to stdout).
        foo_one_push_eval_fn(&mut []);
    }
}
