Feature: Two people finish the same task
  If Mona and Mara both finish a task while they cannot reach each other, both
  completions are kept. Only the two of them can decide which one counts.

  Scenario: Mona settles a double completion with Mara
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Feed the cat"
    And Mona and Mara see the task "Feed the cat"
    When the sync server is stopped
    And Mona finishes "Feed the cat" with the note "Mona fed her"
    And Mara finishes "Feed the cat" with the note "Mara fed her"
    And the sync server is started
    Then Patrick, Mona and Mara see "Feed the cat" finished twice
    And Patrick can only wait for Mona and Mara to decide "Feed the cat"
    When Mona keeps Mara's completion of "Feed the cat"
    Then Patrick, Mona and Mara see "Feed the cat" done by Mara
