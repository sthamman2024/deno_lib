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
async fn op_hello(mut input: DenoLibData) -> DenoLibData {
    println!("Hello {:?}!", input);
    input.id += 1;
    input.name = format!("Hello {}", input.name);
    input
}

async fn make_main_worker() -> anyhow::Result<MainWorker> {
    let mut flags = Flags::default();
    flags.cached_only = false;
    // pass empty vec to allow all
    flags.allow_net = Some(vec![
        "deno.land".to_string(),
        "raw.githubusercontent.com".into(),
    ]);
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
const { op_hello } = Deno.core.ensureFastOps();

async function hello(val) {
  return await op_hello(val);
}

globalThis.flow = { hello };

```

### side.ts

```ts
import { crypto } from "https://deno.land/std@0.184.0/crypto/mod.ts";
import { toHashString } from "https://deno.land/std@0.184.0/crypto/to_hash_string.ts";
import * as dateFns from "https://deno.land/x/date_fns@v2.15.0/index.js";
import { HelloWorld } from "https://raw.githubusercontent.com/deleteman/versioned-deno-module/4.0/hello.ts";

console.log("loaded side.ts");

export async function main() {
  const message = "Hello Deno!";
  const messageBuffer = new TextEncoder().encode(message);
  const hashBuffer = await crypto.subtle.digest("SHA-256", messageBuffer);
  const hash = toHashString(hashBuffer);
  console.log("<p>Hash=" + hash + "</p>");

  console.log(`<p>${dateFns.format(new Date(), "yyyy-MM-dd HH:mm:ss")}</p>`);
  globalThis.val = (globalThis.val ?? 0) + 1;

  const result = await deno_lib.hello({
    id: 1,
    name: "test",
    data: [1, 2, 3],
  });

  console.log("return:", JSON.stringify(result));

  HelloWorld("Deno lib", "bold");
  HelloWorld("Deno lib", "italic");

  return { hash, val: globalThis.val };
}
```

### dependencies

```toml
[dependencies]
anyhow = "1.0.57"
deno_core = { version = "0.194.0" }
deno_lib =  { version = "1.35.0", git = 'https://github.com/sthamman2024/deno_lib.git', branch="lib.1.35.0" }
futures = "0.3.28"
log = "=0.4.17"
serde = { version = "1.0.149", features = ["derive"] }
serde_json = "1.0.85"
tokio = { version = "1.28.1", features = ["full"] }
```