import { definePlugin } from "@alint-js/plugin";

import { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
import { privateSchemaToolkitRule } from "./rules/no-private-schema-toolkit";
import { establishedFoundationRule } from "./rules/prefer-established-foundation";

export { vacantControlBoundaryRule } from "./rules/no-vacant-control-boundary";
export { privateSchemaToolkitRule } from "./rules/no-private-schema-toolkit";
export { establishedFoundationRule } from "./rules/prefer-established-foundation";

export default definePlugin({
  rules: {
    "no-vacant-control-boundary": vacantControlBoundaryRule,
    "no-private-schema-toolkit": privateSchemaToolkitRule,
    "prefer-established-foundation": establishedFoundationRule,
  },
});
