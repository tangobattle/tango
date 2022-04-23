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
      styleOverrides: {
        sizeMedium: {
          height: "40px",
        },
      },
    },
    MuiTabs: {
      styleOverrides: {
        root: {
          minHeight: "0",
        },
      },
    },
    MuiTab: {
      styleOverrides: {
        root: {
          minHeight: "0",
        },
      },
    },
    MuiChip: {
      styleOverrides: {
        root: {
          borderRadius: 4,
          height: "auto",
        },
      },
    },
  },
});

export default theme;
