# Wiki Graph RAG
tags: wiki, graph, rag, retrieval, evidence

Retrieval starts from graph relationships between atoms, recipes, gates, providers, and proof records. Text excerpts support the selected route after graph traversal ranks evidence nodes.

The graph also loads a bounded, deduplicated set of durable learning records and enforces the same 256-node cap for records learned during a live process. Superseded and oldest learning nodes are removed with their edges. Failed records are correction-only evidence and cannot promote a recipe as proof. Successful gate records support their recipe only after their route or SHA-256 artifact evidence verifies. Provider prompts label all retrieved excerpts as untrusted historical data.

[[rag:wiki-graph]]
[[wiki-graph-rag]]
[[wiki:self-learning]]
