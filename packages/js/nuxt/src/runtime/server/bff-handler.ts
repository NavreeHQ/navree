/** The BFF's Nitro route: `/api/**`, `/broadcasting/auth` and any extra mount. */
import { createBffHandler } from "../../server/handlers.js";
import { useNavreeBff } from "./utils.js";

export default createBffHandler(useNavreeBff);
