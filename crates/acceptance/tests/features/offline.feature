Feature: Tackly works without the server and syncs later
  Basic functionality never waits for the network. What happened offline is
  uploaded when the server is reachable, and everyone catches up.

  Background:
    Given Patrick, Mona and Mara have opened Tackly

  Scenario: Patrick uses Tackly before any server exists, then shares it
    Given the sync server is stopped
    When Patrick creates the family "The Smiths"
    And Patrick adds the task "Pack the bags"
    And Patrick starts "Pack the bags"
    And Patrick finishes "Pack the bags" with the note "Done offline"
    Then Patrick sees "Pack the bags" done by Patrick
    And Patrick's app says it is offline
    When the sync server is started
    And Patrick invites Mona
    Then Mona sees "Pack the bags" done by Patrick with a duration, the note "Done offline" and a location

  Scenario: A server outage does not stop anyone, and all three catch up
    Given the sync server is running
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the tasks "Laundry" and "Groceries"
    And Mona and Mara see the tasks "Laundry" and "Groceries"
    When the sync server is stopped
    And Mona starts "Laundry"
    And Mara finishes "Groceries"
    And Patrick adds the task "Vacuum"
    Then Mona's app says it is offline
    When the sync server is started
    Then Patrick, Mona and Mara see "Laundry" in progress by Mona
    And Patrick, Mona and Mara see "Groceries" done by Mara
    And Patrick, Mona and Mara see the task "Vacuum"
    And Mona's app says it is live
