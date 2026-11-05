class BranchesFixture {
    int Run(string s, bool a, bool b) {
        try {
            while (a && b) { s = s ?? "x"; break; }
        } catch (System.Exception) { return 0; }
        return s.Length;
    }
}
