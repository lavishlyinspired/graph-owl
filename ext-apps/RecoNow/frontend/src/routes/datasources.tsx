import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function DatasourcesRoute() {
  return <GenericScreen config={screenConfig("datasources")} />;
}
