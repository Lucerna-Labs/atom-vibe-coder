(function () {
  "use strict";

  const qTierLadder = ["Q2", "Q3", "Q4", "Q5", "Q6", "Q8"];
  const missionInvariant = "Q2/Q3/Q4/Q5/Q6/Q8";
  const data = window.MATH_ATOMS_DATA;
  const storageKey = "math-atoms-coder-state-v1";
  const verdicts = ["BASELINE", "REAL", "PAINTED", "HURTS"];
  const state = loadState();
  const ui = {};

  window.MATH_ATOMS_APP_INVARIANTS = {
    qTierLadder: qTierLadder,
    missionInvariant: missionInvariant
  };

  document.addEventListener("DOMContentLoaded", boot);

  function boot() {
    [
      "atomCount",
      "proofCount",
      "driftCount",
      "intentInput",
      "resetIntent",
      "runLoop",
      "captureRecipe",
      "markDrift",
      "clearSearch",
      "gateList",
      "recipeSearch",
      "recipeList",
      "fabricCanvas",
      "stepGrid",
      "missionGate",
      "benchSelected",
      "benchTable",
      "artifactTitle",
      "artifactSummary",
      "atomSvg",
      "statusValue",
      "artifactProofs",
      "bondCount",
      "atomMap",
      "proofLog"
    ].forEach(function (id) {
      ui[id] = document.getElementById(id);
    });

    ui.runLoop.addEventListener("click", runLoop);
    ui.captureRecipe.addEventListener("click", captureRecipe);
    ui.resetIntent.addEventListener("click", function () {
      ui.intentInput.value = data.defaultIntent;
      runLoop();
    });
    ui.markDrift.addEventListener("click", function () {
      state.drift += 1;
      state.status = "drift flagged";
      addLog("Drift Flagged", "Operator marked the route for review before another proof can be trusted.");
      saveState();
      render();
    });
    ui.clearSearch.addEventListener("click", function () {
      ui.recipeSearch.value = "";
      renderRecipes();
    });
    ui.recipeSearch.addEventListener("input", renderRecipes);
    ui.benchSelected.addEventListener("click", benchSelectedAtom);

    document.querySelectorAll(".tabbar button").forEach(function (button) {
      button.addEventListener("click", function () {
        setTab(button.dataset.tab);
      });
    });
    document.querySelectorAll(".layer-toggle button").forEach(function (button) {
      button.addEventListener("click", function () {
        const layer = button.dataset.layer;
        state.visibleLayers = state.visibleLayers.includes(layer)
          ? state.visibleLayers.filter(function (item) { return item !== layer; })
          : state.visibleLayers.concat(layer);
        saveState();
        renderFabric();
      });
    });

    render();
  }

  function loadState() {
    const fallback = {
      selectedRecipe: "atom-renderer-bootstrap",
      selectedAtom: "measure",
      selectedQ: "Q4",
      status: "draft",
      proofCount: 0,
      drift: 0,
      capturedRecipes: [],
      visibleLayers: ["L0", "L1", "L2", "L3"],
      verdicts: {},
      log: []
    };
    try {
      const raw = localStorage.getItem(storageKey);
      return raw ? Object.assign(fallback, JSON.parse(raw)) : fallback;
    } catch (_error) {
      return fallback;
    }
  }

  function saveState() {
    localStorage.setItem(storageKey, JSON.stringify(state));
  }

  function render() {
    ui.atomCount.textContent = data.atoms.length;
    ui.proofCount.textContent = state.proofCount;
    ui.driftCount.textContent = state.drift;
    ui.artifactProofs.textContent = state.proofCount;
    ui.statusValue.textContent = state.status;
    renderGates();
    renderRecipes();
    renderFabric();
    renderSteps();
    renderMissionGate();
    renderBench();
    renderArtifact();
    renderAtomMap();
    renderProofLog();
  }

  function renderGates() {
    ui.gateList.innerHTML = data.gates.map(function (gate) {
      return '<article class="gate-card"><span class="check" aria-hidden="true">OK</span><span><strong>' +
        clean(gate.title) + "</strong><small>" + clean(gate.body) + '</small></span><span class="layer-pill">' +
        gate.layer + "</span></article>";
    }).join("");
  }

  function renderRecipes() {
    const query = ui.recipeSearch.value.trim().toLowerCase();
    const recipes = allRecipes().filter(function (recipe) {
      const haystack = [recipe.name, recipe.status, recipe.kind, recipe.atoms.join(" ")].join(" ").toLowerCase();
      return haystack.includes(query);
    });
    ui.recipeList.innerHTML = recipes.map(function (recipe) {
      const active = recipe.id === state.selectedRecipe ? " active" : "";
      return '<button class="recipe-card' + active + '" type="button" data-recipe="' + cleanAttr(recipe.id) +
        '"><span class="recipe-icon" aria-hidden="true">A</span><span><strong>' + clean(recipe.name) +
        "</strong><small>" + recipe.level + " / " + recipe.status + " / " + recipe.kind + "</small></span></button>";
    }).join("");
    ui.recipeList.querySelectorAll("[data-recipe]").forEach(function (button) {
      button.addEventListener("click", function () {
        state.selectedRecipe = button.dataset.recipe;
        saveState();
        render();
      });
    });
  }

  function renderFabric() {
    document.querySelectorAll(".layer-toggle button").forEach(function (button) {
      button.classList.toggle("active", state.visibleLayers.includes(button.dataset.layer));
    });

    const width = 510;
    const height = 418;
    const visibleNodes = data.fabric.nodes.filter(function (node) {
      return state.visibleLayers.includes(node.layer);
    });
    const visibleIds = new Set(visibleNodes.map(function (node) { return node.id; }));
    const recipeAtoms = new Set(selectedRecipe().atoms);
    const nodeById = {};
    data.fabric.nodes.forEach(function (node) { nodeById[node.id] = node; });

    const layerLines = data.fabric.layers.filter(function (layer) {
      return state.visibleLayers.includes(layer.id);
    }).map(function (layer) {
      return '<line class="layer-guide" x1="0" y1="' + layer.y + '" x2="' + width + '" y2="' + layer.y +
        '"></line><text class="layer-label" x="14" y="' + (layer.y - 8) + '">' + clean(layer.label) + "</text>";
    }).join("");

    const links = data.fabric.links.filter(function (link) {
      return visibleIds.has(link[0]) && visibleIds.has(link[1]);
    }).map(function (link) {
      const a = nodeById[link[0]];
      const b = nodeById[link[1]];
      const active = recipeAtoms.has("flow") || recipeAtoms.has("compose") ? " active" : "";
      const midX = (a.x + b.x) / 2;
      return '<path class="fabric-link' + active + '" d="M ' + a.x + " " + a.y + " C " +
        midX + " " + a.y + ", " + midX + " " + b.y + ", " + b.x + " " + b.y + '"></path>';
    }).join("");

    const nodes = visibleNodes.map(function (node) {
      const selected = node.id === state.focusNode ? 5 : 0;
      return '<g class="fabric-node" data-node="' + cleanAttr(node.id) + '"><circle cx="' + node.x +
        '" cy="' + node.y + '" r="' + (17 + selected) + '" fill="' + node.color +
        '"></circle><text x="' + node.x + '" y="' + (node.y + 4) + '" text-anchor="middle" fill="#fff">' +
        node.layer + '</text><text x="' + node.x + '" y="' + (node.y + 35) + '" text-anchor="middle">' +
        clean(node.label) + "</text></g>";
    }).join("");

    ui.fabricCanvas.innerHTML = '<svg viewBox="0 0 ' + width + " " + height +
      '" role="img" aria-label="Spiderweb fabric route">' + layerLines + links + nodes + "</svg>";
    ui.fabricCanvas.querySelectorAll("[data-node]").forEach(function (node) {
      node.addEventListener("click", function () {
        state.focusNode = node.dataset.node;
        addLog("Fabric Node Selected", node.dataset.node + " is now the active inspection point.");
        saveState();
        render();
      });
    });
  }

  function renderSteps() {
    const recipe = selectedRecipe();
    const done = new Set(["intent-atom", "recipe-retrieval", "artifact-preview"]);
    if (state.proofCount !== 0) {
      done.add("proof-run");
      done.add("store-learning");
    }
    if (recipe.atoms.includes("compare") || recipe.atoms.includes("measure")) {
      done.add("self-critique");
    }
    if (recipe.atoms.includes("compose")) {
      done.add("molecule-build");
    }
    ui.stepGrid.innerHTML = data.steps.map(function (step) {
      const status = done.has(step.id) ? " done" : step.id === "patch-reaction" ? " warn" : "";
      return '<article class="step-card' + status + '"><strong>' + clean(step.title) +
        "</strong><small>" + clean(step.body) + "</small></article>";
    }).join("");
  }

  function renderMissionGate() {
    const known = data.operatorMission.recipes.map(function (recipe) { return recipe.key; });
    const full = qTierLadder.every(function (tier) { return known.includes(tier); });
    const buttons = qTierLadder.map(function (tier) {
      const active = tier === state.selectedQ ? " active" : "";
      return '<button type="button" class="' + active + '" data-q="' + tier + '" title="Select ' +
        tier + '">' + tier + "</button>";
    }).join("");
    ui.missionGate.innerHTML = "<strong>" + clean(data.operatorMission.title) + "</strong><small>" +
      clean(data.operatorMission.body) + "</small><small>Ladder: " + missionInvariant + " / " +
      (full ? "preserved" : "blocked") + '</small><div class="q-grid">' + buttons + "</div>";
    ui.missionGate.querySelectorAll("[data-q]").forEach(function (button) {
      button.addEventListener("click", function () {
        state.selectedQ = button.dataset.q;
        state.selectedRecipe = "ds4-q-recipe-selector";
        addLog("Q Recipe Selected", state.selectedQ + " is the active candidate until evidence gates pass or fail.");
        saveState();
        render();
      });
    });
  }

  function renderBench() {
    ui.benchTable.innerHTML = data.atoms.filter(function (atom) {
      return atom.layer === "extended";
    }).map(function (atom) {
      const verdict = state.verdicts[atom.key] || "BASELINE";
      return '<article class="bench-row" data-verdict="' + verdict + '"><span><strong>' + atom.id +
        ". " + clean(atom.key) + "</strong><small>" + clean(atom.currency) + " Risk: " +
        clean(atom.risk) + '.</small></span><button type="button" data-bench="' + cleanAttr(atom.key) +
        '" title="Cycle verdict">' + verdict + "</button></article>";
    }).join("");
    ui.benchTable.querySelectorAll("[data-bench]").forEach(function (button) {
      button.addEventListener("click", function () { cycleVerdict(button.dataset.bench); });
    });
  }

  function renderArtifact() {
    const recipe = selectedRecipe();
    const atoms = recipe.atoms.map(atomByKey).filter(Boolean);
    const positions = [[140, 170], [292, 135], [320, 285], [188, 300], [230, 95], [102, 258]];
    const classes = ["electron-teal", "electron-amber", "electron-red", "electron-blue"];
    const dots = atoms.map(function (atom, index) {
      const point = positions[index % positions.length];
      return '<circle class="' + classes[index % classes.length] + '" cx="' + point[0] + '" cy="' +
        point[1] + '" r="10"><title>' + clean(atom.key) + "</title></circle>";
    }).join("");

    ui.artifactTitle.textContent = recipe.name;
    ui.artifactSummary.textContent = recipe.summary;
    ui.bondCount.textContent = recipe.bonds;
    ui.statusValue.textContent = recipe.status === "proven" && state.proofCount !== 0 ? "proven" : state.status;
    ui.atomSvg.innerHTML = '<ellipse class="orbit" cx="210" cy="210" rx="128" ry="54" transform="rotate(22 210 210)"></ellipse>' +
      '<ellipse class="orbit" cx="210" cy="210" rx="128" ry="54" transform="rotate(-24 210 210)"></ellipse>' +
      '<circle class="nucleus" cx="210" cy="210" r="40"></circle><text class="atom-label" x="210" y="213">' +
      recipe.level + "</text>" + dots;
  }

  function renderAtomMap() {
    const selected = new Set(selectedRecipe().atoms);
    ui.atomMap.innerHTML = data.atoms.map(function (atom) {
      const chosen = selected.has(atom.key) ? " selected" : "";
      return '<button class="atom-card ' + atom.layer + chosen + '" type="button" data-atom="' +
        cleanAttr(atom.key) + '"><strong>' + atom.id + ". " + clean(atom.key) + "</strong><small>" +
        clean(atom.summary) + " " + clean(atom.currency) + "</small></button>";
    }).join("");
    ui.atomMap.querySelectorAll("[data-atom]").forEach(function (button) {
      button.addEventListener("click", function () {
        state.selectedAtom = button.dataset.atom;
        saveState();
        render();
      });
    });
  }

  function renderProofLog() {
    const recipe = selectedRecipe();
    const base = [
      { title: "Selected Recipe", body: recipe.name + " binds " + recipe.atoms.join(", ") + "." },
      { title: "Required Gate", body: "The route keeps proof required, artifact visible, and fail-closed gates active." }
    ];
    ui.proofLog.innerHTML = state.log.concat(base).slice(0, 9).map(function (item) {
      return '<article class="proof-card"><strong>' + clean(item.title) + "</strong><p>" +
        clean(item.body) + "</p></article>";
    }).join("");
  }

  function runLoop() {
    const intent = ui.intentInput.value.trim() || data.defaultIntent;
    const atoms = classifyIntent(intent);
    const recipe = chooseRecipe(atoms);
    state.selectedRecipe = recipe.id;
    state.status = "proven";
    state.proofCount += 1;
    addLog("Loop Proof", "Matched " + atoms.map(function (atom) { return atom.key; }).join(", ") + " from the current intent.");
    addLog("Recipe Route", recipe.name + " selected before generic fallback.");
    saveState();
    render();
  }

  function captureRecipe() {
    const intent = ui.intentInput.value.trim() || data.defaultIntent;
    const atoms = classifyIntent(intent).map(function (atom) { return atom.key; });
    const recipe = {
      id: "captured-" + Date.now(),
      name: titleFromIntent(intent),
      level: atoms.some(function (key) { return atomByKey(key).layer === "extended"; }) ? "L3" : "L2",
      status: "proven",
      kind: "captured",
      summary: intent,
      atoms: atoms,
      bonds: Math.max(1, atoms.length - 1)
    };
    state.capturedRecipes.unshift(recipe);
    state.selectedRecipe = recipe.id;
    state.status = "proven";
    state.proofCount += 1;
    addLog("Recipe Captured", recipe.name + " was added to the local recipe store.");
    saveState();
    render();
  }

  function benchSelectedAtom() {
    const key = state.selectedAtom || selectedRecipe().atoms[0] || "transform";
    const atom = atomByKey(key);
    if (!atom || atom.layer !== "extended") {
      addLog("Bench Baseline", key + " is a stable root atom; no extended verdict required.");
    } else {
      cycleVerdict(key);
      addLog("Bench Run", atom.key + " checked against currency: " + atom.currency);
    }
    saveState();
    render();
  }

  function cycleVerdict(key) {
    const current = state.verdicts[key] || "BASELINE";
    const next = verdicts[(verdicts.indexOf(current) + 1) % verdicts.length];
    state.verdicts[key] = next;
    state.selectedAtom = key;
    addLog("Verdict Updated", key + " moved from " + current + " to " + next + ".");
    saveState();
    render();
  }

  function classifyIntent(intent) {
    const lower = intent.toLowerCase();
    const scored = data.atoms.map(function (atom) {
      return {
        atom: atom,
        score: atom.keywords.reduce(function (total, keyword) {
          return lower.includes(keyword) ? total + 1 : total;
        }, 0)
      };
    }).filter(function (item) {
      return item.score !== 0;
    }).sort(function (a, b) {
      return b.score - a.score || a.atom.id - b.atom.id;
    }).map(function (item) {
      return item.atom;
    });
    const fallback = ["scan", "project", "compare", "compose", "measure"].map(atomByKey);
    return uniqueAtoms(scored.length ? scored : fallback).slice(0, 6);
  }

  function chooseRecipe(atoms) {
    const atomKeys = new Set(atoms.map(function (atom) { return atom.key; }));
    return allRecipes().map(function (recipe) {
      const overlap = recipe.atoms.filter(function (key) { return atomKeys.has(key); }).length;
      return { recipe: recipe, score: overlap * 2 + (recipe.status === "proven" ? 1 : 0) };
    }).sort(function (a, b) {
      return b.score - a.score || b.recipe.bonds - a.recipe.bonds;
    })[0].recipe;
  }

  function allRecipes() {
    return state.capturedRecipes.concat(data.recipes);
  }

  function selectedRecipe() {
    return allRecipes().find(function (recipe) { return recipe.id === state.selectedRecipe; }) || data.recipes[0];
  }

  function atomByKey(key) {
    return data.atoms.find(function (atom) { return atom.key === key; });
  }

  function uniqueAtoms(atoms) {
    const seen = new Set();
    return atoms.filter(function (atom) {
      if (!atom || seen.has(atom.key)) {
        return false;
      }
      seen.add(atom.key);
      return true;
    });
  }

  function titleFromIntent(intent) {
    const words = intent.replace(/[^a-z0-9 ]/gi, " ").split(/\s+/).filter(Boolean).slice(0, 4);
    return words.map(function (word) {
      return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
    }).join(" ") || "Captured Recipe";
  }

  function setTab(tab) {
    document.querySelectorAll(".tabbar button").forEach(function (button) {
      button.classList.toggle("active", button.dataset.tab === tab);
    });
    document.querySelectorAll(".tab-panel").forEach(function (panel) {
      panel.classList.toggle("active", panel.id === tab + "Tab");
    });
  }

  function addLog(title, body) {
    state.log.unshift({ title: title, body: body });
    state.log = state.log.slice(0, 16);
  }

  function clean(value) {
    return String(value)
      .split("&").join("&amp;")
      .split("<").join("&lt;")
      .split('"').join("&quot;")
      .split("'").join("&#39;");
  }

  function cleanAttr(value) {
    return clean(value).split("`").join("&#96;");
  }
})();
