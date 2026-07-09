use math_atoms_core::{provider_output_hash, MathAtomsRuntime, ProviderConfig, RuntimeStatus};

fn main() {
    let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_process_env());
    let run = runtime.run_intent(
        "Run the configured provider model against wiki graph RAG evidence on the Spiderweb Bus.",
    );
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
