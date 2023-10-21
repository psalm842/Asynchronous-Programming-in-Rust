use std::error::Error;
use std::fmt::Write as WriteFmt;
use std::fs;
use std::io::Write;

const FN_KW: &str = "dude";
const W_KW: &str = "chill";

fn main() {
    let mainrs = fs::read_to_string("./main.rs").unwrap();
    // Truncate main.rs and rewrite what's there, we'll rewrite it
    let mut new = fs::File::create("./hello.rs").unwrap();

    if !mainrs.starts_with("// REWRITE") {
        return;
    }
    // remove rewrite line in the start
    let mainrs: String = mainrs.lines().skip(1).map(|l| format!("{l}\n")).collect();

    // Find the start point of async blocks
    let start_points = find_kw_start_points(&mainrs);

    // No keywords, no async functions, do nothing
    if start_points.is_empty() {
        return;
    }

    let mut async_start_end = vec![];

    for start in start_points {
        // find the end
        // Find the end of the async function
        let mut brackets_counter = 0;
        let mut end = start;

        for char in mainrs[start..].chars() {
            end += 1;
            match char {
                '{' => brackets_counter += 1,
                '}' => {
                    brackets_counter -= 1;
                    if brackets_counter == 0 {
                        break;
                    }
                }
                _ => (),
            }
        }

        // store the locations
        async_start_end.push((start, end));
    }

    // Write everything except the async functions back to the file
    // (we put the rewritten code last in the file since it's easier
    // to see)

    let mut pos_tracker = 0;
    for (start, end) in &async_start_end {
        new.write_all(&mainrs[pos_tracker..*start].as_bytes())
            .unwrap();
        pos_tracker = *end;
    }
    // Write everything after the last async fn
    new.write_all(&mainrs[pos_tracker..].as_bytes()).unwrap();

    // transform the async functions and write them to the file

    for (i, (start, end)) in async_start_end.into_iter().enumerate() {
        let id = i.to_string();

        let async_fn = String::from(&mainrs[start..end - 1]);

        // transfrom the async fn
        let transformed = transform(&async_fn, &id);

        // Write the coroutine implementation to file
        new.write_all(transformed.as_bytes()).unwrap();
    }
}

fn find_kw_start_points(s: &str) -> Vec<usize> {
    let mut start_points = vec![];
    let mut index = 0;

    for line in s.lines() {
        // Remove everything that's commented
        let (txt, _) = match line.split_once("//") {
            Some((txt, commented)) => (txt, commented),
            None => (line, ""),
        };

        // Search in the text that's not commented
        match txt.find(FN_KW) {
            Some(kw_start) => {
                start_points.push(index + kw_start);
                index += line.len() + 1;
            }

            // remember that index is 0-based, len is 1-based,
            // but line does not include `\n` so the number ends up equal
            None => {
                // An empty line will only be a `\n`
                let len = if line.len() == 0 { 1 } else { line.len() + 1 };
                index += len;
            }
        }
    }
    start_points
}

// Transforms an async function into a state machine, "mimmicing"
// what happens when compiling an async function in Rust
fn transform(async_fn: &str, id: &str) -> String {
    // first Comment out the async function
    let commented = comment_orig(&async_fn);
    // Then  rewrite the async function itself
    let (args, new_async_fn) = create_new_async_fn(&async_fn, &id);
    // Rewrite the async function to a state machine
    let rewritten = rewrite_async_fn(&async_fn, &id, args).unwrap();
    format!("{commented}{new_async_fn}{rewritten}")
}

/// Format and comment out the original "async" function
fn comment_orig(orig: &str) -> String {
    let mut res = String::new();
    writeln!(
        &mut res,
        "

// =================================
// We rewrite this:
// =================================
    "
    )
    .unwrap();
    for line in orig.lines() {
        writeln!(&mut res, "// {line}").unwrap();
    }
    writeln!(
        &mut res,
        "
// }}

// =================================
// Into this:
// =================================
"
    )
    .unwrap();

    res
}

// Returns the new async function and it's arguments
fn create_new_async_fn(func: &str, coro_id: &str) -> (Vec<(String, String)>, String) {
    // first line is expected to be the function definition `keyword fn name() -> ReturnType`
    // remove the keyword
    let def = &func.lines().nth(0).unwrap()[FN_KW.len() + 1..];

    // get the name of the function
    let (fn_name, arg_start) = def.split_once("(").expect("Expected `(`").clone();
    let (args, _) = arg_start.split_once(")").expect("Expected `)`");

    let args = get_args(&args);

    let (_, res_type) = def.split_once(")").expect("Expected `)`");
    // clean up res_type to somethig we can use and check if there is one defined at all

    let res_type = if res_type.trim().eq("{") {
        "()".to_string()
    } else {
        let (_, t) = res_type.split_once("->").expect("Expected `->`");
        t.trim().to_string()
    };

    let args_fmt = format_args_name_and_types(&args);
    let arg_names = format_args_names_only(&args);

    let new_async_fn = format!(
        "{fn_name}({args_fmt}) -> impl Future<Output={res_type}> {{
    Coroutine{coro_id}::new({arg_names})
}}
        "
    );

    (args, new_async_fn)
}

/// Rewrite the async function (this is very brittle, but does
/// the job for our example)
fn rewrite_async_fn(
    s: &str,
    id: &str,
    args: Vec<(String, String)>,
) -> Result<String, Box<dyn Error>> {
    let w_kw_len = W_KW.len();

    // Store the code in each "step" in this variable
    let mut steps = vec![];
    // Store the future call that we yield on
    let mut futures = vec![];

    let mut buffer = String::new();
    // Skip the first line since that's the function definition
    for line in s.lines().skip(1) {
        // If the line contains the keyword it's an await-point
        if line.contains(W_KW) {
            // Store the steps since last await point as a "step"
            steps.push(buffer.clone());
            buffer.clear();
            // Remove the keyword itself (i.e. `.await`)
            let l = &line.trim_end()[..line.len() - 1 - w_kw_len - 1];
            // we need both the future call and the variable name since
            // we most likely reference this variable name in the next "step"
            // This could be:
            // `let txt = Http::get("...").await`
            // or simply
            // `join_all(futures).await`
            match l.split_once("=") {
                Some((var, fut)) => {
                    // This could fail in so many ways...
                    let varname = &var[var.find("let").unwrap() + 3..].trim();
                    futures.push((varname.to_string(), fut.to_string()));
                }
                None => futures.push(("_".to_string(), l.trim().to_string())),
            }

            // We store the variable name and the future as a tuple since they're connected
        } else {
            buffer.push_str(line);
            buffer.push_str("\n");
        }
    }

    steps.push(buffer);

    // Write our steps enum. We know it will start with "Start" and end with "Resolved"
    // but we need to add one step for each await point

    let step_args = format_args_types_only(&args);

    let mut steps_enum = format!(
        "
enum State{id} {{
    Start{step_args},"
    );

    // We only support this kind of future
    for i in 1..steps.len() {
        write!(
            &mut steps_enum,
            "
    Wait{i}(Box<dyn Future<Output = String>>),"
        )?;
    }

    write!(
        &mut steps_enum,
        "
    Resolved,
}}"
    )?;

    // So, our `State` enum is finished, we create a coroutine struct and a simple
    // `new` implementation
    let coro_args = format_args_name_and_types(&args);
    let coro_args_names = format_args_names_only(&args);

    let coroutine = format!(
        "
struct Coroutine{id} {{
    state: State{id},
}}

impl Coroutine{id} {{
    fn new({coro_args}) -> Self {{
        Self {{ state: State{id}::Start{coro_args_names} }}
    }}
}}
"
    );

    // This is our future implementation
    let mut imp = format!(
        "
impl Future for Coroutine{id} {{
    type Output = ();

    fn poll(&mut self) -> PollState<()> {{"
    );

    for (i, step) in steps.iter().enumerate() {
        // This is the index for the next step in the state machine
        let next = i + 1;

        // We need to special case the first call since that
        // happens before we reach an `await` point

        // This will recieve the input args to the function
        let impl_fut_first_args = format_args_names_only(&args);

        if i == 0 {
            let futname = &futures[i].1;
            write!(
                &mut imp,
                "
        match self.state {{
            State{id}::Start{impl_fut_first_args} => {{
                // ---- Code you actually wrote ----
            {step}
                // ---------------------------------
                let fut{next} = Box::new({futname});
                self.state = State{id}::Wait{next}(fut{next});
                PollState::NotReady
            }}
"
            )?;

        // These steps are await-ponts where we await a future
        } else if i < steps.len() - 1 {
            let varname = &futures[i - 1].0;
            let fut = &futures[i].1;
            write!(
                &mut imp,
                "
            State{id}::Wait{i}(ref mut f{i}) => {{
                match f{i}.poll() {{
                    PollState::Ready({varname}) => {{
                        // ---- Code you actually wrote ----
                    {step}
                        // ---------------------------------
                        let fut{next} = Box::new({fut});
                        self.state = State{id}::Wait{next}(fut{next});
                        PollState::NotReady
                    }}
                    PollState::NotReady => PollState::NotReady,
                }}
            }}
"
            )?;

        // This is the part after the last await point. There is no need to yield any more
        } else {
            let varname = &futures[i - 1].0;
            write!(
                &mut imp,
                "
            State{id}::Wait{i}(ref mut f{i}) => {{
                match f{i}.poll() {{
                    PollState::Ready({varname}) => {{
                        // ---- Code you actually wrote ----
                    {step}
                        // ---------------------------------
                        self.state = State{id}::Resolved;
                        PollState::Ready(())
                    }}
                    PollState::NotReady => PollState::NotReady,
                }}
            }}
"
            )?;
        }
    }

    // If we poll the future after it has resolved, we panic
    writeln!(
        &mut imp,
        "
            State{id}::Resolved => panic!(\"Polled a resolved future\")
        }}
    }}
}}"
    )?;

    // Format the different parts of the Coroutine implementation to a string
    Ok(format!("{steps_enum}\n{coroutine}\n{imp}"))
}

// this expects something like `txt: String, i: usize` or an empty string
fn get_args(s: &str) -> Vec<(String, String)> {
    let mut res = vec![];
    if s.trim().is_empty() {
        return res;
    }

    let args = s.split(",");
    for arg in args {
        let (argname, ty) = arg.split_once(":").expect("Expected `:`");
        res.push((argname.trim().to_string(), ty.trim().to_string()));
    }

    res
}

/// Gets:
/// `&[(txt, String), (i: usize)]`
/// Outputs
/// `(String, usize)`
/// If there are no args it returns: ""
fn format_args_types_only(args: &[(String, String)]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let mut args_fmt: String = args.iter().map(|(_n, ty)| format!("{ty},")).collect();
        // remove last `,`
        args_fmt.pop();
        format!("({args_fmt})")
    }
}

/// Gets:
/// `&[(txt, String), (i: usize)]`
/// Outputs
/// `txt: String, i: usize`
/// If there are no args it returns: ""
fn format_args_name_and_types(args: &[(String, String)]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let mut args_fmt: String = args.iter().map(|(n, ty)| format!("{n}: {ty},")).collect();
        // remove last `,`
        args_fmt.pop();
        format!("{args_fmt}")
    }
}

/// Gets:
/// `&[(txt, String), (i: usize)]`
/// Outputs
/// `(txt, i)`
/// If there are no args it returns: ""
fn format_args_names_only(args: &[(String, String)]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let mut args_fmt: String = args.iter().map(|(n, _ty)| format!("{n},")).collect();
        // remove last `,`
        args_fmt.pop();
        format!("({args_fmt})")
    }
}
