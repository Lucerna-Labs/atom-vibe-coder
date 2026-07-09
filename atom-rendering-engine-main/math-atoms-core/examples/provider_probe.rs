use math_atoms_core::{provider_output_hash, MathAtomsRuntime, ProviderConfig, RuntimeStatus};

fn main() {
    let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_process_env());
    let arg_intent = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let intent = if !arg_intent.trim().is_empty() {
        arg_intent
    } else {
        std::env::var("MATH_ATOMS_PROVIDER_PROBE_INTENT").unwrap_or_else(|_| {
            "Run the configured provider model against wiki graph RAG evidence on the Spiderweb Bus."
                .to_string()
        })
    };
    let run = runtime.run_intent(&intent);
    if run.status == RuntimeStatus::Blocked {
        eprintln!(
            "provider proof blocked before execution: {:?}",
            run.blockers
        );
        std::process::exit(2);
    }
    if run.provider_call.is_none() {
        eprintln!("provider proof blocked: no provider call was prepared");
        std::process::exit(3);
    }
    let Some(task) = runtime.schedule_provider_execution() else {
        eprintln!(
            "provider proof blocked during Spiderweb scheduling: {:?}",
            runtime.state().blockers
        );
        std::process::exit(3);
    };
    match task.call.execute_with_curl() {
        Ok(text) if !text.trim().is_empty() => {
            let output_hash = provider_output_hash(&text);
            runtime.mark_provider_executed(&output_hash, text.len());
            println!("provider execution ok: {} chars", text.chars().count());
            println!("{}", text.trim());
        }
        Ok(_) => {
            eprintln!("provider proof blocked: provider returned empty text");
            std::process::exit(4);
        }
        Err(error) => {
            runtime.mark_provider_blocked(&format!("{error:?}"));
            eprintln!("provider execution blocked: {error:?}");
            std::process::exit(5);
        }
    }
}
