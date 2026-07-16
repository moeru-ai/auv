import { definePlugin } from "@alint-js/plugin";

import { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
import { establishedFoundationRule } from "./rules/prefer-established-foundation";

export { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
export { establishedFoundationRule } from "./rules/prefer-established-foundation";

export default definePlugin({
  rules: {
    "no-vacant-control-boundary": vacantControlBoundaryRule,
    "prefer-established-foundation": establishedFoundationRule,
  },
});
