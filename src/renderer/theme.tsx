import green from "@mui/material/colors/green";
import { createTheme } from "@mui/material/styles";

const theme = createTheme({
  typography: {
    button: {
      fontWeight: "bold",
      textTransform: "none",
    },
  },
  palette: {
    primary: green,
  },
  components: {
    MuiButton: {
      defaultProps: {
        disableElevation: true,
      },
    },
  },
});

export default theme;
