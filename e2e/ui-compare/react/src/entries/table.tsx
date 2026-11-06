import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "../styles.css"
import { TableScreen } from "../screens/table"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TableScreen />
  </StrictMode>
)
