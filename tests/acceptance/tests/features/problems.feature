Feature: Problems
  Things that go wrong at home again and again. Noticed for oneself, or raised
  for someone else; each solution becomes a step. Resolved by hand.

  Scenario: Patrick raises "Laundry piles up" for Mona; she works on it and resolves it
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick raises "Laundry piles up" for Mona
    Then Patrick, Mona and Mara see the problem "Laundry piles up"
    And Mona's Problems tab shows 1
    And Patrick's Problems tab shows 0
    When Mona opens the problem "Laundry piles up"
    Then Mona sees the tasks "Wash a load", "Hang it up" and "Put it away"
    When Mona ticks "Wash a load" off
    And Mona resolves it
    Then Mona is told "Resolved: Laundry piles up"
    And Patrick, Mona and Mara no longer see the problem "Laundry piles up"
    And Mona's Problems tab shows 0

  Scenario: Patrick makes a problem of his own and gives it to Mara
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick makes the new problem "Car is dirty" with the solution "Wash the car" and notices it
    Then Patrick sees the problem "Car is dirty"
    And Patrick's Problems tab shows 1
    When Patrick gives the problem "Car is dirty" to Mara
    Then Mara's Problems tab shows 1
    And Patrick's Problems tab shows 0
    When Mara opens the problem "Car is dirty"
    Then Mara sees the task "Wash the car"

  Scenario: Resolved by mistake: Undo brings it back
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick notices "Plants are dry"
    And Patrick opens the problem "Plants are dry"
    And Patrick resolves it
    And Patrick undoes "Resolved: Plants are dry"
    Then Patrick and Mona see the problem "Plants are dry"
