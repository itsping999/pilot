import { realpathSync } from "node:fs";
import { access, readFile, realpath, stat } from "node:fs/promises";
import { relative, isAbsolute } from "node:path";
import {
  loadSkillsFromDir,
  type Skill,
  createReadToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { ApiError, badRequest } from "./errors.js";

export function contained(root: string, target: string): boolean {
  const path = relative(root, target);
  return path === "" || (!path.startsWith("..") && !isAbsolute(path));
}

export class Skills {
  readonly root: string;
  readonly entries: Skill[];
  constructor(directory: string) {
    try {
      this.root = realpathSync(directory);
      const result = loadSkillsFromDir({
        dir: this.root,
        source: "agent-service",
      });
      if (result.diagnostics.length)
        throw new Error("Invalid skill definition");
      this.entries = result.skills;
      const names = new Set<string>();
      for (const skill of this.entries) {
        if (
          !contained(this.root, realpathSync(skill.filePath)) ||
          !/^[a-z0-9][a-z0-9-]{0,63}$/.test(skill.name) ||
          names.has(skill.name)
        )
          throw new Error("Invalid skill");
        names.add(skill.name);
      }
    } catch {
      throw new ApiError(
        500,
        "configuration_error",
        "技能目录或技能定义无效。",
      );
    }
  }

  catalogue() {
    return this.entries.map((skill) => ({
      name: skill.name,
      description: skill.description,
      manual_only: skill.disableModelInvocation,
    }));
  }

  validate(name: string): void {
    if (!this.entries.some((entry) => entry.name === name))
      throw badRequest("指定的技能不存在。");
  }

  readTool(workspace: string) {
    const check = async (filename: string): Promise<string> => {
      try {
        const actual = await realpath(filename);
        const work = await realpath(workspace);
        if (!contained(this.root, actual) && !contained(work, actual))
          throw new Error("Unavailable");
        const info = await stat(actual);
        if (!info.isFile() || info.size > 1024 * 1024)
          throw new Error("Unavailable");
        return actual;
      } catch {
        throw new Error("File is not available to this conversation.");
      }
    };
    return createReadToolDefinition(workspace, {
      operations: {
        access: async (filename) => {
          await access(await check(filename));
        },
        readFile: async (filename) => readFile(await check(filename)),
        detectImageMimeType: async () => null,
      },
    });
  }
}
