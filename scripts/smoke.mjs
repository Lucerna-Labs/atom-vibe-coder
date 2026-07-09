import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const root = process.cwd();
const operatorMission = "match or beat ds4 by selecting the smallest gate-passing q2/q3/q4/q5/q6/q8 recipe from clean full-precision evidence";
const files = [
  "README.md",
  "app/index.html",
  "app/styles.css",
  "app/app-data.js",
  "app/app.js",
  "atom-extension-16.md"
];

for (const file of files) {
  const absolute = path.join(root, file);
  if (!fs.existsSync(absolute)) {
    throw new Error(`Missing required file: ${file}`);
  }
}

const html = fs.readFileSync(path.join(root, "app/index.html"), "utf8");
for (const token of ["Math Atoms Coder", "app-data.js", "app.js", "Spiderweb Build Layer"]) {
  if (!html.includes(token)) {
    throw new Error(`index.html missing token: ${token}`);
  }
}

const context = {
  window: {},
  document: {
    addEventListener() {}
  },
  localStorage: {
    getItem() {
      return null;
    },
    setItem() {}
  }
};
vm.createContext(context);
vm.runInContext(fs.readFileSync(path.join(root, "app/app-data.js"), "utf8"), context);
vm.runInContext(fs.readFileSync(path.join(root, "app/app.js"), "utf8"), context);

const data = context.window.MATH_ATOMS_DATA;
const invariants = context.window.MATH_ATOMS_APP_INVARIANTS;
if (!data || !Array.isArray(data.atoms)) {
  throw new Error("MATH_ATOMS_DATA.atoms is missing");
}
if (data.atoms.length !== 16) {
  throw new Error(`Expected 16 atoms, found ${data.atoms.length}`);
}
if (data.atoms.filter((atom) => atom.layer === "root").length !== 8) {
  throw new Error("Expected 8 root atoms");
}
if (data.atoms.filter((atom) => atom.layer === "extended").length !== 8) {
  throw new Error("Expected 8 extended atoms");
}
for (const atom of data.atoms.filter((item) => item.layer === "extended")) {
  if (!atom.testIdea || !atom.currency || !atom.risk) {
    throw new Error(`Extended atom lacks bench metadata: ${atom.key}`);
  }
}
if (!data.fabric || data.fabric.layers.length !== 4) {
  throw new Error("Spiderweb fabric must expose four layers");
}

const expectedLadder = ["Q2", "Q3", "Q4", "Q5", "Q6", "Q8"];
const dataLadder = data.operatorMission.recipes.map((recipe) => recipe.key).join("/");
const appLadder = invariants.qTierLadder.join("/");
if (dataLadder !== expectedLadder.join("/") || appLadder !== expectedLadder.join("/")) {
  throw new Error("Q2/Q3/Q4/Q5/Q6/Q8 ladder was not preserved");
}
if (!data.operatorMission.body.toLowerCase().includes(operatorMission)) {
  throw new Error("Operator mission text was not preserved");
}

console.log("smoke ok: static app, atom doctrine, Spiderweb layers, Q ladder, and operator mission validated");
