import { definePlugin } from "@alint-js/core";
import todo from "./rules/todo";

export default definePlugin({
  rules: {
    todo,
  },
});
