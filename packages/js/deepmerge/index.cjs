"use strict";

// Recursive merge of plain objects and arrays. The default behaviour and the
// options (`arrayMerge`, `clone`, `customMerge`, `isMergeableObject`, and
// `all`) follow deepmerge 4, which Nitro's bundler loads.
//
// A key that reaches or replaces a prototype (`__proto__`, `constructor`,
// `prototype`) is never copied, from either side and whatever the target
// holds, so no input can change what `Object.prototype` or the result's
// prototype carries. A merge follows at most `maxDepth` levels of nesting, so a
// self-referencing or very deep input is an error, not a stack overflow, and
// does at most `maxWork` steps (one for each object and array it copies and
// each key it reads), so objects that share their children, which a walk
// takes for a tree of 2^depth nodes, are an error too and not a hang.

const maxDepth = 1000;
const maxWork = 5000000;

// How deep the merges running now have gone. Merging is synchronous, so one
// counter serves every call, nested ones (from a custom merge) included.
let depth = 0;

// The steps the merge in progress has done, counted from its outermost call.
let work = 0;

function spend(steps) {
  work += steps;
  if (work > maxWork) {
    throw new RangeError(`deepmerge: the merge needs more than ${maxWork} steps of work`);
  }
}

const reactElementType = typeof Symbol === "function" && Symbol.for ? Symbol.for("react.element") : 0xeac7;

function isMergeableObject(value) {
  if (!value || typeof value !== "object") return false;
  const tag = Object.prototype.toString.call(value);
  return tag !== "[object RegExp]" && tag !== "[object Date]" && value.$$typeof !== reactElementType;
}

function emptyTarget(value) {
  return Array.isArray(value) ? [] : {};
}

function cloneUnlessOtherwiseSpecified(value, options) {
  return options.clone !== false && options.isMergeableObject(value) ? merge(emptyTarget(value), value, options) : value;
}

function defaultArrayMerge(target, source, options) {
  spend(target.length + source.length);
  return target.concat(source).map((element) => cloneUnlessOtherwiseSpecified(element, options));
}

function getMergeFunction(key, options) {
  if (!options.customMerge) return merge;
  const customMerge = options.customMerge(key);
  return typeof customMerge === "function" ? customMerge : merge;
}

function ownKeys(target) {
  const keys = Object.keys(target);
  for (const symbol of Object.getOwnPropertySymbols(target)) {
    if (Object.prototype.propertyIsEnumerable.call(target, symbol)) keys.push(symbol);
  }
  return keys;
}

function propertyIsOnObject(object, property) {
  try {
    return property in object;
  } catch {
    return false;
  }
}

// A property is unsafe to merge when the target has it up its prototype chain
// or as a non-enumerable property: the merge would act on something that is
// not the target's own data.
function propertyIsUnsafe(target, key) {
  return (
    propertyIsOnObject(target, key) &&
    !(Object.prototype.hasOwnProperty.call(target, key) && Object.prototype.propertyIsEnumerable.call(target, key))
  );
}

function keyIsForbidden(key) {
  return key === "__proto__" || key === "constructor" || key === "prototype";
}

function mergeObject(target, source, options) {
  const destination = {};
  if (options.isMergeableObject(target)) {
    const targetKeys = ownKeys(target);
    spend(targetKeys.length);
    for (const key of targetKeys) {
      if (keyIsForbidden(key)) continue;
      destination[key] = cloneUnlessOtherwiseSpecified(target[key], options);
    }
  }
  const sourceKeys = ownKeys(source);
  spend(sourceKeys.length);
  for (const key of sourceKeys) {
    if (keyIsForbidden(key) || propertyIsUnsafe(target, key)) continue;
    destination[key] =
      propertyIsOnObject(target, key) && options.isMergeableObject(source[key])
        ? getMergeFunction(key, options)(target[key], source[key], options)
        : cloneUnlessOtherwiseSpecified(source[key], options);
  }
  return destination;
}

function merge(target, source, options) {
  options = options || {};
  options.arrayMerge = options.arrayMerge || defaultArrayMerge;
  options.isMergeableObject = options.isMergeableObject || isMergeableObject;
  // Added to `options` so that a custom `arrayMerge` can use it.
  options.cloneUnlessOtherwiseSpecified = cloneUnlessOtherwiseSpecified;

  if (depth >= maxDepth) {
    throw new RangeError(`deepmerge: nesting is deeper than ${maxDepth} levels`);
  }
  if (depth === 0) work = 0;
  spend(1);
  depth += 1;
  try {
    const sourceIsArray = Array.isArray(source);
    if (sourceIsArray !== Array.isArray(target)) return cloneUnlessOtherwiseSpecified(source, options);
    return sourceIsArray ? options.arrayMerge(target, source, options) : mergeObject(target, source, options);
  } finally {
    depth -= 1;
  }
}

merge.all = function all(array, options) {
  if (!Array.isArray(array)) {
    throw new Error("first argument should be an array");
  }
  return array.reduce((previous, next) => merge(previous, next, options), {});
};

merge.maxDepth = maxDepth;
merge.maxWork = maxWork;

module.exports = merge;
