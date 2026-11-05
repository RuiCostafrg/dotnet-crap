class BranchesFixture {
    /// <summary>Exercise loop, Boolean, coalescing and catch decisions with explicit probes.</summary>
    int Run(string s, bool a, bool b) {
        try {
            while (a && b) { Hits.Mark("Branches.cs", 5); s = s ?? throw new System.Exception(); break; }
        } catch (System.Exception) { Hits.Mark("Branches.cs", 6); return 0; }
        return s.Length;
    }
}
