class BranchesFixture {
    int Run(string s, bool a, bool b) {
        try {
            while (a && b) { Hits.Mark("Branches.cs", 4); s = s ?? throw new System.Exception(); break; }
        } catch (System.Exception) { Hits.Mark("Branches.cs", 5); return 0; }
        return s.Length;
    }
}
