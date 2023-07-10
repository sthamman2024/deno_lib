# Deno

A fork of Deno to allow the cli (specifically the module loader) to be embedded in rust.

### Sample usage

```rust
use deno_core::op;
use deno_core::resolve_path;
use deno_lib::deno_runtime::deno_core::serde_v8;
use deno_lib::deno_runtime::permissions::Permissions;
use deno_lib::deno_runtime::permissions::PermissionsContainer;
use deno_lib::deno_runtime::worker::MainWorker;
use deno_lib::deno_runtime::worker::WorkerOptions;
use deno_lib::js::deno_isolate_init;
use deno_lib::CliFactory;
use deno_lib::Flags;
use serde::{Deserialize, Serialize};
use std::env::current_dir;

#[derive(Debug, Serialize, Deserialize)]
pub struct DenoLibData {
    pub id: i32,
    pub name: String,
    pub data: Vec<u8>,
}

deno_core::extension!(
  deno_lib,
  ops = [op_hello],
  esm_entry_point = "ext:deno_lib/bootstrap.js",
  esm = [dir "src", "bootstrap.js"],
);

#[op]
fn op_hello(mut input: DenoLibData) -> DenoLibData {
    println!("Hello {:?}!", input);
    input.id += 1;
    input.name = format!("Hello {}", input.name);
    input
}

async fn make_main_worker() -> anyhow::Result<MainWorker> {
    let mut flags = Flags::default();
    flags.cached_only = false;
    flags.allow_net = Some(vec![]);
    flags.cache_path = Some(".deno_lib".to_string().into());
    flags.allow_read = Some(vec!["./local".to_string().into()]);
    flags.no_prompt = true;
    flags.allow_env = None;
    flags.log_level = Some(log::Level::Error);

    let factory = CliFactory::from_flags(flags.clone()).await?;
    let worker_factory = factory.create_cli_main_worker_factory().await?;
    let cli_options = factory.cli_options();
    let permissions = PermissionsContainer::new(Permissions::from_options(
        &cli_options.permissions_options(),
    )?);
    let module_loader = worker_factory.create_module_loader(permissions.clone());
    let cwd = current_dir()?;
    let main_module = resolve_path("./main.ts", &cwd)?;

    let worker = MainWorker::bootstrap_from_options(
        main_module.clone(),
        permissions,
        WorkerOptions {
            module_loader: module_loader.clone(),
            extensions: vec![deno_lib::init_ops_and_esm()], // add your custom extensions
            startup_snapshot: Some(deno_isolate_init()),
            ..WorkerOptions::default()
        },
    );
    Ok(worker)
}

async fn run_script() -> anyhow::Result<()> {
    let mut worker = make_main_worker().await?;
    let script = r#"import("./local/side.ts").then(async ({main}) => {
        const result = await main();
        return result;
    })
    "#;
    let global = worker.execute_script("<script_name>", script.to_owned().into())?;
    let global = worker.js_runtime.resolve_value(global).await?;
    let mut scope = worker.js_runtime.handle_scope();
    let local_var = deno_core::v8::Local::new(&mut scope, global);
    let val = serde_v8::from_v8::<serde_json::Value>(&mut scope, local_var)
        .expect("Unable to deserialize");
    println!("value: {:?}", val);
    Ok(())
}
```

### bootstrap.js

```js
function hello(val) {
  return Deno[Deno.internal].core.ops.op_hello(val);
}
globalThis.deno_lib = { hello };
```

### dependencies

```toml
[dependencies]
anyhow = "1.0.57"
deno_core = { version = "0.195.0" }
deno_lib =  { version = "1.35.0", git = 'https://github.com/sthamman2024/deno_lib.git', branch="lib" }
futures = "0.3.28"
log = "=0.4.17"
serde = { version = "1.0.149", features = ["derive"] }
serde_json = "1.0.85"
tokio = { version = "1.28.1", features = ["full"] }
```
