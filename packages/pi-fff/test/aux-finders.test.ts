import { describe, expect, test } from "bun:test";
import { resolveAuxRoot, routePathConstraint } from "../src/aux-finders";

describe("routePathConstraint", () => {
  const cwd = "/tmp/workspace";

  test("returns null for relative paths", () => {
    expect(routePathConstraint("src/", cwd)).toBeNull();
    expect(routePathConstraint("**/*.ts", cwd)).toBeNull();
    expect(routePathConstraint(undefined, cwd)).toBeNull();
  });

  test("returns null for absolute paths inside the workspace", () => {
    expect(routePathConstraint("/tmp/workspace/src", cwd)).toBeNull();
  });

  test("routes absolute paths outside the workspace", () => {
    const route = routePathConstraint("/tmp", cwd);
    expect(route).toEqual({ root: "/tmp", suffix: "" });
  });

  test("splits glob suffix from existing dir prefix", () => {
    const route = routePathConstraint("/tmp/**/*.ts", cwd);
    expect(route).toEqual({ root: "/tmp", suffix: "**/*.ts" });
  });

  test("returns null when prefix does not exist on disk", () => {
    expect(routePathConstraint("/tmp/__nonexistent_dir_xyz__", cwd)).toBeNull();
  });
});

describe("resolveAuxRoot", () => {
  test("returns null for relative input", () => {
    expect(resolveAuxRoot("src/")).toBeNull();
  });
});
